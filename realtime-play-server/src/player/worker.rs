use std::{sync::Arc, time::Duration};

use cmrt_core::{CoreConfig, LiveMidiEvent, RealtimeRenderer};
use cpal::traits::StreamTrait;

use super::{
    audio_output::{AudioOutputConsumer, AudioOutputControl, AudioOutputProducer},
    output_stream::build_output_stream,
    LiveQueuedEvent, PlaybackMode, PlayerCommand, PlayerInner,
};

const OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(10);

/// live キューに積める未発音イベントの上限。暴走クライアントでメモリを食い潰さないための蓋。
/// BPM130 の16分音符で 16 行なら1ステップ最大32イベントなので、64ステップ先まで積める勘定。
const MAX_LIVE_QUEUE_EVENTS: usize = 2048;

pub(super) fn run_player_worker(
    inner: Arc<PlayerInner>,
    audio_output: Arc<AudioOutputControl>,
    core_cfg: CoreConfig,
    plugin_path: String,
    output_producer: AudioOutputProducer,
    output_consumer: AudioOutputConsumer,
    init_tx: std::sync::mpsc::Sender<std::result::Result<(), String>>,
) {
    let entry = match cmrt_core::load_entry(&plugin_path) {
        Ok(entry) => entry,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    let mut renderer = match RealtimeRenderer::new(&core_cfg, &entry) {
        Ok(renderer) => renderer,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    let output_stream = match build_output_stream(output_consumer, core_cfg.sample_rate) {
        Ok(stream) => stream,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    if let Err(error) = output_stream.play() {
        let _ = init_tx.send(Err(format!("オーディオ出力の開始失敗: {error}")));
        return;
    }
    let _ = init_tx.send(Ok(()));

    let _keep_stream_alive = output_stream;
    let mut playback_mode: Option<PlaybackMode> = None;
    loop {
        if playback_mode.is_none() {
            let Some(command) = inner.wait_for_command() else {
                break;
            };
            apply_command(&mut renderer, &mut playback_mode, command);
            continue;
        }
        if let Some(command) = inner.pop_pending_command() {
            apply_command(&mut renderer, &mut playback_mode, command);
            continue;
        }
        if !output_producer.wait_for_space_timeout(OUTPUT_WAIT_TIMEOUT) {
            continue;
        }

        let Some(mode) = playback_mode.as_mut() else {
            continue;
        };
        match mode {
            PlaybackMode::Scheduled {
                generation,
                playback,
            } => match renderer.render_next_chunk(playback) {
                Ok(Some(chunk)) => {
                    if !output_producer.push_chunk(*generation, chunk) {
                        playback_mode = None;
                    }
                }
                Ok(None) => {
                    audio_output.finish();
                    playback_mode = None;
                }
                Err(error) => {
                    eprintln!("realtime play process failed: {error:#}");
                    renderer.reset();
                    audio_output.finish();
                    playback_mode = None;
                }
            },
            PlaybackMode::Live {
                generation,
                clock_samples,
                queue,
            } => {
                let buf_size = renderer.buf_size() as u64;
                let chunk_start = *clock_samples;
                let events = take_chunk_events(queue, chunk_start, buf_size);
                match renderer.render_live_chunk_with_offsets(&events) {
                    Ok(chunk) => {
                        *clock_samples = chunk_start + buf_size;
                        let _ = output_producer.push_chunk(*generation, chunk);
                    }
                    Err(error) => {
                        eprintln!("realtime live MIDI process failed: {error:#}");
                        renderer.reset();
                        audio_output.finish();
                        playback_mode = None;
                    }
                }
            }
        }
    }
}

/// patch 切替のたびに live session を作り直す。切替で音は切れるため `clock_samples` は 0 で良い。
fn new_live_mode(generation: u64) -> PlaybackMode {
    PlaybackMode::Live {
        generation,
        clock_samples: 0,
        queue: Vec::new(),
    }
}

/// live キューへ `at_sample` 昇順を保って積む。同じ位置のイベントは受け取った順を保つ。
///
/// `offsets` は空（全て 0）か `messages` と同数。上限を超えたぶんは捨てる。
pub(super) fn enqueue_live_events(
    queue: &mut Vec<LiveQueuedEvent>,
    clock_samples: u64,
    messages: &[[u8; 3]],
    offsets: &[u32],
) {
    for (index, message) in messages.iter().enumerate() {
        if queue.len() >= MAX_LIVE_QUEUE_EVENTS {
            eprintln!(
                "realtime live MIDI queue is full ({MAX_LIVE_QUEUE_EVENTS} events); dropping the rest"
            );
            return;
        }
        let offset = offsets.get(index).copied().unwrap_or(0);
        let at_sample = clock_samples.saturating_add(u64::from(offset));
        // 同位置のイベントの後ろへ挿す（note off → note on の順序が保たれる）。
        let insert_at = queue.partition_point(|queued| queued.at_sample <= at_sample);
        queue.insert(
            insert_at,
            LiveQueuedEvent {
                at_sample,
                message: *message,
            },
        );
    }
}

/// このチャンクで鳴らすイベントをキューから取り出し、chunk 内オフセットへ変換する。
///
/// チャンク開始より過去のイベント（遅刻ぶん）は捨てずにオフセット 0 へクランプする。
pub(super) fn take_chunk_events(
    queue: &mut Vec<LiveQueuedEvent>,
    chunk_start: u64,
    buf_size: u64,
) -> Vec<LiveMidiEvent> {
    let chunk_end = chunk_start.saturating_add(buf_size);
    let last_frame = buf_size.saturating_sub(1);
    let take = queue.partition_point(|queued| queued.at_sample < chunk_end);
    queue
        .drain(..take)
        .map(|queued| LiveMidiEvent {
            offset_frames: queued.at_sample.saturating_sub(chunk_start).min(last_frame) as u32,
            message: queued.message,
        })
        .collect()
}

fn apply_command(
    renderer: &mut RealtimeRenderer,
    playback_mode: &mut Option<PlaybackMode>,
    command: PlayerCommand,
) {
    match command {
        PlayerCommand::Play {
            generation,
            schedule,
            patch,
        } => {
            renderer.reset();
            if let Err(error) = renderer.set_patch(patch.as_deref()) {
                eprintln!("realtime play patch load failed: {error:#}");
            }
            *playback_mode = Some(PlaybackMode::Scheduled {
                generation,
                playback: schedule,
            });
        }
        PlayerCommand::Stop { generation } => {
            let _ = generation;
            renderer.reset();
            *playback_mode = None;
        }
        PlayerCommand::Midi {
            generation,
            messages,
            offsets,
            patch,
            enter_live,
        } => {
            if enter_live || !matches!(playback_mode, Some(PlaybackMode::Live { .. })) {
                renderer.reset();
                if let Err(error) = renderer.set_patch(patch.as_deref()) {
                    eprintln!("realtime live MIDI patch load failed: {error:#}");
                }
                *playback_mode = Some(new_live_mode(generation));
            }
            if let Some(PlaybackMode::Live {
                generation: live_generation,
                clock_samples,
                queue,
            }) = playback_mode
            {
                *live_generation = generation;
                enqueue_live_events(queue, *clock_samples, &messages, &offsets);
            }
        }
        PlayerCommand::PrepareLivePatch {
            generation,
            patch,
            completion,
        } => {
            renderer.reset();
            match renderer.set_patch(patch.as_deref()) {
                Ok(()) => {
                    *playback_mode = Some(new_live_mode(generation));
                    let _ = completion.send(Ok(()));
                }
                Err(error) => {
                    *playback_mode = None;
                    let _ = completion.send(Err(format!("{error:#}")));
                }
            }
        }
        PlayerCommand::ProbeLivePatch {
            generation,
            patch,
            completion,
        } => {
            renderer.reset();
            let result = renderer
                .set_patch(patch.as_deref())
                .and_then(|()| renderer.probe_voicing());
            match result {
                Ok(report) => {
                    *playback_mode = Some(new_live_mode(generation));
                    let _ = completion.send(Ok(report));
                }
                Err(error) => {
                    renderer.reset();
                    *playback_mode = None;
                    let _ = completion.send(Err(format!("{error:#}")));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
