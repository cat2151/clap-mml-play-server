use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use cmrt_core::{CoreConfig, LiveMidiEvent, RealtimeRenderer};
use cpal::traits::StreamTrait;

use crate::timing;

use super::{
    audio_output::{AudioOutputConsumer, AudioOutputControl, AudioOutputProducer},
    auto_gain::target_rms_db,
    limiter::MasterLimiter,
    mixer::add_samples_ramped,
    output_stream::build_output_stream,
    runtime::{new_live_instances, AutoGainControl, LiveGains, LiveInstanceState, PlaybackMode},
    startup::create_live_renderers,
    LimiterMeterState, LiveQueuedEvent, PlayerCommand, PlayerInner,
};

const OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(10);
const MAX_LIVE_QUEUE_EVENTS: usize = 2048;
/// patch state のロード後、note on を受け付けられる状態になるまで空回しするブロック数。
///
/// probe 経路の `PATCH_SETTLE_BLOCKS`（core-lib/src/render/voicing_probe.rs）と同じ目的。
/// state をロードしただけの Surge XT へ即 note on を送ると、パラメータ再構築と同じ
/// process ブロックに乗ってしまい握り潰されることがある。
const PATCH_SETTLE_BLOCKS: usize = 4;

pub(super) struct WorkerOutput {
    pub(super) control: Arc<AudioOutputControl>,
    pub(super) limiter_meter: Arc<LimiterMeterState>,
    pub(super) live_gains: Arc<LiveGains>,
    pub(super) auto_gain: Arc<AutoGainControl>,
    pub(super) producer: AudioOutputProducer,
    pub(super) consumer: AudioOutputConsumer,
}

pub(super) fn run_player_worker(
    inner: Arc<PlayerInner>,
    output: WorkerOutput,
    core_cfg: CoreConfig,
    plugin_path: String,
    live_instance_count: usize,
    init_tx: std::sync::mpsc::Sender<std::result::Result<(), String>>,
) {
    let WorkerOutput {
        control: audio_output,
        limiter_meter,
        live_gains,
        auto_gain,
        producer: output_producer,
        consumer: output_consumer,
    } = output;
    let mut renderers = match create_live_renderers(&core_cfg, &plugin_path, live_instance_count) {
        Ok(renderers) => renderers,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };

    let audio_stream_started = Instant::now();
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
    timing::log_phase("audio_stream", audio_stream_started.elapsed());
    let _ = init_tx.send(Ok(()));

    let _keep_stream_alive = output_stream;
    let mut limiter = MasterLimiter::new(core_cfg.sample_rate);
    let auto_gain_target_db = target_rms_db(live_instance_count);
    let mut playback_mode: Option<PlaybackMode> = None;
    loop {
        if playback_mode.is_none() {
            let Some(command) = inner.wait_for_command() else {
                break;
            };
            apply_command(
                &mut renderers,
                &mut limiter,
                &limiter_meter,
                &audio_output,
                &mut playback_mode,
                command,
            );
            continue;
        }
        if let Some(command) = inner.pop_pending_command() {
            apply_command(
                &mut renderers,
                &mut limiter,
                &limiter_meter,
                &audio_output,
                &mut playback_mode,
                command,
            );
            continue;
        }
        if !output_producer.wait_for_space_timeout(OUTPUT_WAIT_TIMEOUT) {
            continue;
        }

        let render_result = match playback_mode.as_mut() {
            Some(PlaybackMode::Scheduled {
                generation,
                playback,
            }) => renderers[0]
                .render_next_chunk(playback)
                .map(|chunk| chunk.map(|samples| (*generation, samples))),
            Some(PlaybackMode::Live {
                generation,
                clock_samples,
                instances,
            }) => render_live_mix(
                &mut renderers,
                instances,
                clock_samples,
                &live_gains,
                auto_gain.enabled(),
                core_cfg.sample_rate,
                auto_gain_target_db,
            )
            .map(|samples| Some((*generation, samples))),
            None => continue,
        };

        match render_result {
            Ok(Some((generation, mut samples))) => {
                let reduction = limiter.process(&mut samples);
                limiter_meter.update(reduction.current_db, reduction.peak_db);
                if !output_producer.push_chunk(generation, samples) {
                    playback_mode = None;
                }
            }
            Ok(None) => {
                audio_output.finish();
                limiter.reset();
                limiter_meter.reset();
                playback_mode = None;
            }
            Err(error) => {
                eprintln!("realtime play process failed: {error:#}");
                reset_all(&mut renderers);
                limiter.reset();
                limiter_meter.reset();
                audio_output.finish();
                playback_mode = None;
            }
        }
    }
}

fn render_live_mix(
    renderers: &mut [RealtimeRenderer],
    instances: &mut [LiveInstanceState],
    clock_samples: &mut u64,
    gains: &LiveGains,
    auto_gain_enabled: bool,
    sample_rate: f64,
    auto_gain_target_db: f32,
) -> anyhow::Result<Vec<f32>> {
    let buf_size = renderers[0].buf_size() as u64;
    let chunk_start = *clock_samples;
    let mut mixed = vec![0.0f32; buf_size as usize * 2];
    for (index, (renderer, instance)) in renderers.iter_mut().zip(instances.iter_mut()).enumerate()
    {
        if !instance.active {
            continue;
        }
        let events = take_chunk_events(&mut instance.queue, chunk_start, buf_size);
        match renderer.render_live_chunk_with_offsets(&events) {
            Ok(samples) => {
                let auto_gain = instance.auto_gain.process_block(
                    &samples,
                    sample_rate,
                    auto_gain_target_db,
                    auto_gain_enabled,
                );
                add_samples_ramped(&mut mixed, &samples, auto_gain.scaled(gains.get(index)));
            }
            Err(error) => {
                eprintln!("realtime live instance {index} failed: {error:#}");
                renderer.reset();
                instance.active = false;
                instance.queue.clear();
                instance.auto_gain.reset();
            }
        }
    }
    *clock_samples = chunk_start + buf_size;
    Ok(mixed)
}

fn new_live_mode(generation: u64, instance_count: usize) -> PlaybackMode {
    PlaybackMode::Live {
        generation,
        clock_samples: 0,
        instances: new_live_instances(instance_count),
    }
}

pub(super) fn enqueue_live_event(
    queue: &mut Vec<LiveQueuedEvent>,
    clock_samples: u64,
    event: cmrt_realtime_ipc::FastMidiEvent,
) {
    if queue.len() >= MAX_LIVE_QUEUE_EVENTS {
        eprintln!(
            "realtime live MIDI queue is full ({MAX_LIVE_QUEUE_EVENTS} events); dropping event"
        );
        return;
    }
    let at_sample = clock_samples.saturating_add(u64::from(event.offset_frames));
    let insert_at = queue.partition_point(|queued| queued.at_sample <= at_sample);
    queue.insert(
        insert_at,
        LiveQueuedEvent {
            at_sample,
            message: event.message,
        },
    );
}

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
    renderers: &mut [RealtimeRenderer],
    limiter: &mut MasterLimiter,
    limiter_meter: &LimiterMeterState,
    audio_output: &AudioOutputControl,
    playback_mode: &mut Option<PlaybackMode>,
    command: PlayerCommand,
) {
    match command {
        PlayerCommand::Play {
            generation,
            schedule,
            patch,
        } => {
            reset_all(renderers);
            limiter.reset();
            limiter_meter.reset();
            if let Err(error) = renderers[0].set_patch(patch.as_deref()) {
                eprintln!("realtime play patch load failed: {error:#}");
            }
            *playback_mode = Some(PlaybackMode::Scheduled {
                generation,
                playback: schedule,
            });
        }
        PlayerCommand::StopAll { generation } => {
            let _ = generation;
            reset_all(renderers);
            limiter.reset();
            limiter_meter.reset();
            *playback_mode = None;
        }
        PlayerCommand::StopInstance {
            generation,
            instance_id,
        } => {
            ensure_live_mode(playback_mode, generation, renderers.len());
            let instance_index = usize::from(instance_id);
            renderers[instance_index].reset();
            if let Some(PlaybackMode::Live {
                generation: live_generation,
                instances,
                ..
            }) = playback_mode
            {
                *live_generation = generation;
                instances[instance_index].active = false;
                instances[instance_index].queue.clear();
                instances[instance_index].auto_gain.reset();
                if instances.iter().all(|instance| !instance.active) {
                    audio_output.finish();
                    limiter.reset();
                    limiter_meter.reset();
                    *playback_mode = None;
                }
            }
        }
        PlayerCommand::Midi {
            generation,
            events,
            enter_live,
        } => {
            if enter_live || !matches!(playback_mode, Some(PlaybackMode::Live { .. })) {
                reset_all(renderers);
                limiter.reset();
                limiter_meter.reset();
                *playback_mode = Some(new_live_mode(generation, renderers.len()));
            }
            if let Some(PlaybackMode::Live {
                generation: live_generation,
                clock_samples,
                instances,
            }) = playback_mode
            {
                *live_generation = generation;
                for event in events {
                    let instance = &mut instances[usize::from(event.instance_id)];
                    instance.active = true;
                    enqueue_live_event(&mut instance.queue, *clock_samples, event);
                }
            }
        }
        PlayerCommand::PrepareLivePatch {
            generation,
            instance_id,
            patch,
            completion,
        } => {
            ensure_live_mode(playback_mode, generation, renderers.len());
            let index = usize::from(instance_id);
            renderers[index].reset();
            let result = renderers[index]
                .set_patch(patch.as_deref())
                .and_then(|()| settle_patch(&mut renderers[index]))
                .map_err(|error| format!("{error:#}"));
            if let Some(PlaybackMode::Live {
                generation: live_generation,
                instances,
                ..
            }) = playback_mode
            {
                *live_generation = generation;
                instances[index].queue.clear();
                instances[index].auto_gain.reset();
                // 成功時に `active` を立てない。立てると、鳴らす予定のない instance まで
                // 毎チャンク process() を回すことになる。grid sequencer の chord mode は
                // 演奏の裏で待機 bank を仕込むので、そこが無音かつ CPU ゼロで居られること
                // に依存している。最初の MIDI イベントが届いた時点で `Midi` 側が立てる。
                if result.is_err() {
                    instances[index].active = false;
                }
                if result.is_err() && instances.iter().all(|instance| !instance.active) {
                    audio_output.finish();
                    limiter.reset();
                    limiter_meter.reset();
                    *playback_mode = None;
                }
            }
            let _ = completion.send(result);
        }
        PlayerCommand::ProbeLivePatch {
            generation,
            instance_id,
            patch,
            completion,
        } => {
            ensure_live_mode(playback_mode, generation, renderers.len());
            let index = usize::from(instance_id);
            renderers[index].reset();
            let result = renderers[index]
                .set_patch(patch.as_deref())
                .and_then(|()| renderers[index].probe_voicing())
                .map_err(|error| format!("{error:#}"));
            if let Some(PlaybackMode::Live {
                generation: live_generation,
                instances,
                ..
            }) = playback_mode
            {
                *live_generation = generation;
                instances[index].queue.clear();
                instances[index].auto_gain.reset();
                instances[index].active = result.is_ok();
                if result.is_err() && instances.iter().all(|instance| !instance.active) {
                    audio_output.finish();
                    limiter.reset();
                    limiter_meter.reset();
                    *playback_mode = None;
                }
            }
            let _ = completion.send(result);
        }
    }
}

/// patch のロード直後に空のブロックを回し、プラグインが note on を鳴らせる状態になるまで待つ。
///
/// 進むのは**そのインスタンスのプラグイン内部時刻だけ**で、mix クロック（`clock_samples`）とは
/// `PATCH_SETTLE_BLOCKS × buf_size` ずれる。発音位置はチャンク内 offset で決まるため
/// 発音タイミングには影響せず、ずれるのは LFO やディレイの位相だけ。
/// 生成した音は捨てる（差し替え中のインスタンスは鳴っていない）。
fn settle_patch(renderer: &mut RealtimeRenderer) -> anyhow::Result<()> {
    for _ in 0..PATCH_SETTLE_BLOCKS {
        renderer.render_live_chunk_with_offsets(&[])?;
    }
    Ok(())
}

fn ensure_live_mode(
    playback_mode: &mut Option<PlaybackMode>,
    generation: u64,
    instance_count: usize,
) {
    if !matches!(playback_mode, Some(PlaybackMode::Live { .. })) {
        *playback_mode = Some(new_live_mode(generation, instance_count));
    }
}

fn reset_all(renderers: &mut [RealtimeRenderer]) {
    for renderer in renderers {
        renderer.reset();
    }
}

#[cfg(test)]
mod tests;
