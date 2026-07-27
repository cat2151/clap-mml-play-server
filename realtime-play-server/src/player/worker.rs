use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use cmrt_core::{CoreConfig, LiveMidiEvent, RealtimeRenderer, RendererCreated};
use cpal::traits::StreamTrait;

use crate::timing;

use super::{
    audio_output::{AudioOutputConsumer, AudioOutputControl, AudioOutputProducer},
    limiter::MasterLimiter,
    new_live_instances,
    output_stream::build_output_stream,
    LimiterMeterState, LiveQueuedEvent, PlaybackMode, PlayerCommand, PlayerInner,
};

const OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(10);
const MAX_LIVE_QUEUE_EVENTS: usize = 2048;

pub(super) struct WorkerOutput {
    pub(super) control: Arc<AudioOutputControl>,
    pub(super) limiter_meter: Arc<LimiterMeterState>,
    pub(super) producer: AudioOutputProducer,
    pub(super) consumer: AudioOutputConsumer,
}

/// CLAP インスタンス1つ分の生成内訳を出力する。
///
/// 起動時間のほぼ全部がインスタンス生成なので、どの操作が支配的かをここで切り分ける。
fn log_instance_created(created: RendererCreated) {
    // クライアント（realtime-play/src/process.rs の parse_server_startup_progress）が
    // パースしている行。書式を変えないこと。
    eprintln!(
        "cmrt-server-startup: instances={}/{}",
        created.completed, created.total
    );
    let timing = created.timing;
    timing::log(&format!(
        "phase=instance index={} ms={} plugin_id_ms={} instantiate_ms={} \
         save_state_ms={} load_patch_ms={} activate_ms={} start_processing_ms={}",
        created.index,
        timing.total.as_millis(),
        timing.plugin_id.as_millis(),
        timing.instantiate.as_millis(),
        timing.save_state.as_millis(),
        timing.load_patch.as_millis(),
        timing.activate.as_millis(),
        timing.start_processing.as_millis(),
    ));
}

/// 生成スレッド数を明示指定する環境変数。`1` を入れると従来どおりの逐次生成に戻る。
/// 並列生成は CLAP の main-thread 規約を破っているので、将来の Surge XT で壊れたときの逃げ道。
const BUILD_THREADS_ENV: &str = "CMRT_INSTANCE_BUILD_THREADS";

/// インスタンス生成に使うスレッド数。
///
/// Surge XT の `init()` はほぼ完全に CPU 律速なので、論理コア数まで並べれば実時間が縮む。
/// 生成数より多くのスレッドを作っても意味がないので、そこで頭打ちにする。
fn instance_build_threads() -> usize {
    let default = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    std::env::var(BUILD_THREADS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .unwrap_or(default)
        .min(cmrt_realtime_ipc::INSTANCE_COUNT)
}

pub(super) fn run_player_worker(
    inner: Arc<PlayerInner>,
    output: WorkerOutput,
    core_cfg: CoreConfig,
    plugin_path: String,
    init_tx: std::sync::mpsc::Sender<std::result::Result<(), String>>,
) {
    let WorkerOutput {
        control: audio_output,
        limiter_meter,
        producer: output_producer,
        consumer: output_consumer,
    } = output;
    let load_entry_started = Instant::now();
    let entry = match cmrt_core::load_entry(&plugin_path) {
        Ok(entry) => entry,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    timing::log_phase("load_entry", load_entry_started.elapsed());

    let instances_started = Instant::now();
    let threads = instance_build_threads();
    let mut renderers = match cmrt_core::create_renderers_parallel(
        &core_cfg,
        &entry,
        cmrt_realtime_ipc::INSTANCE_COUNT,
        threads,
        &log_instance_created,
    ) {
        Ok(renderers) => renderers,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    timing::log(&format!(
        "phase=instances_total ms={} count={} threads={threads}",
        instances_started.elapsed().as_millis(),
        cmrt_realtime_ipc::INSTANCE_COUNT
    ));

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
            }) => render_live_mix(&mut renderers, instances, clock_samples)
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
    instances: &mut [super::LiveInstanceState],
    clock_samples: &mut u64,
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
            Ok(samples) => add_samples(&mut mixed, &samples),
            Err(error) => {
                eprintln!("realtime live instance {index} failed: {error:#}");
                renderer.reset();
                instance.active = false;
                instance.queue.clear();
            }
        }
    }
    *clock_samples = chunk_start + buf_size;
    Ok(mixed)
}

fn add_samples(mixed: &mut [f32], samples: &[f32]) {
    for (output, sample) in mixed.iter_mut().zip(samples) {
        *output += *sample;
    }
}

fn new_live_mode(generation: u64) -> PlaybackMode {
    PlaybackMode::Live {
        generation,
        clock_samples: 0,
        instances: new_live_instances(),
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
            ensure_live_mode(playback_mode, generation);
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
                *playback_mode = Some(new_live_mode(generation));
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
            ensure_live_mode(playback_mode, generation);
            let index = usize::from(instance_id);
            renderers[index].reset();
            let result = renderers[index]
                .set_patch(patch.as_deref())
                .map_err(|error| format!("{error:#}"));
            if let Some(PlaybackMode::Live {
                generation: live_generation,
                instances,
                ..
            }) = playback_mode
            {
                *live_generation = generation;
                instances[index].queue.clear();
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
        PlayerCommand::ProbeLivePatch {
            generation,
            instance_id,
            patch,
            completion,
        } => {
            ensure_live_mode(playback_mode, generation);
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

fn ensure_live_mode(playback_mode: &mut Option<PlaybackMode>, generation: u64) {
    if !matches!(playback_mode, Some(PlaybackMode::Live { .. })) {
        *playback_mode = Some(new_live_mode(generation));
    }
}

fn reset_all(renderers: &mut [RealtimeRenderer]) {
    for renderer in renderers {
        renderer.reset();
    }
}

#[cfg(test)]
mod tests;
