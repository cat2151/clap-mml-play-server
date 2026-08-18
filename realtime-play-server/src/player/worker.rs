use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use cmrt_clack_timeline::process_block_timing;
use cmrt_core::{CoreConfig, LiveMidiEvent, RealtimeRenderer};
use cmrt_timeline::{BlockSpan, FreeRunningTimeline, LateEventPolicy, SamplePosition, SampleRate};
use cpal::traits::StreamTrait;

use crate::timing;

use super::{
    audio_output::{AudioOutputConsumer, AudioOutputControl, AudioOutputProducer},
    auto_gain::target_rms_db,
    limiter::MasterLimiter,
    mixer::add_samples_ramped,
    output_stream::build_output_stream,
    runtime::{
        new_live_instances, AutoGainControl, LiveGains, LiveInstanceState, LiveTimelineState,
        PlaybackMode, TimingMetricsState,
    },
    startup::create_live_renderers,
    timing_diagnostics::LiveTimingWindow,
    LimiterMeterState, LiveQueuedEvent, PlayerCommand, PlayerInner,
};

const OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(10);
const MAX_LIVE_QUEUE_EVENTS: usize = 8192;
mod command;

#[cfg(test)]
use command::apply_live_tempo;
use command::{apply_command, reset_all, CommandContext};

pub(super) struct WorkerOutput {
    pub(super) control: Arc<AudioOutputControl>,
    pub(super) limiter_meter: Arc<LimiterMeterState>,
    pub(super) live_gains: Arc<LiveGains>,
    pub(super) auto_gain: Arc<AutoGainControl>,
    pub(super) timing_metrics: Arc<TimingMetricsState>,
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
        timing_metrics,
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
    let mut timing_window = LiveTimingWindow::new(core_cfg.sample_rate);
    loop {
        let waiting_for_timeline_events = matches!(
            playback_mode,
            Some(PlaybackMode::Live {
                timeline: Some(ref timeline),
                ..
            }) if !timeline.started
        );
        if playback_mode.is_none() || waiting_for_timeline_events {
            let Some(command) = inner.wait_for_command() else {
                break;
            };
            apply_command(
                CommandContext {
                    renderers: &mut renderers,
                    limiter: &mut limiter,
                    limiter_meter: &limiter_meter,
                    auto_gain: &auto_gain,
                    timing_window: &mut timing_window,
                    timing_metrics: &timing_metrics,
                    audio_output: &audio_output,
                    playback_mode: &mut playback_mode,
                },
                command,
            );
            continue;
        }
        if let Some(command) = inner.pop_pending_command() {
            apply_command(
                CommandContext {
                    renderers: &mut renderers,
                    limiter: &mut limiter,
                    limiter_meter: &limiter_meter,
                    auto_gain: &auto_gain,
                    timing_window: &mut timing_window,
                    timing_metrics: &timing_metrics,
                    audio_output: &audio_output,
                    playback_mode: &mut playback_mode,
                },
                command,
            );
            continue;
        }
        if !output_producer.wait_for_space_timeout(OUTPUT_WAIT_TIMEOUT) {
            continue;
        }

        let render_started = Instant::now();
        let live_block = matches!(playback_mode, Some(PlaybackMode::Live { .. }));
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
                timeline,
            }) => render_live_mix(
                &mut renderers,
                instances,
                clock_samples,
                LiveMixControls {
                    gains: &live_gains,
                    auto_gain: &auto_gain,
                    sample_rate: core_cfg.sample_rate,
                    auto_gain_target_db,
                },
                timeline.as_mut(),
                &mut timing_window,
            )
            .map(|samples| Some((*generation, samples))),
            None => continue,
        };

        match render_result {
            Ok(Some((generation, mut samples))) => {
                let render_elapsed = render_started.elapsed();
                let reduction = limiter.process(&mut samples);
                limiter_meter.update(reduction.current_db, reduction.peak_db);
                if !output_producer.push_chunk(generation, samples) {
                    playback_mode = None;
                } else if live_block {
                    let block_duration = Duration::from_secs_f64(
                        renderers[0].buf_size() as f64 / core_cfg.sample_rate,
                    );
                    timing_window.observe_block(
                        render_elapsed,
                        block_duration,
                        output_producer.lead_frames() as u64,
                    );
                    timing_window.publish_if_due(&timing_metrics, Instant::now());
                }
            }
            Ok(None) => {
                audio_output.finish();
                limiter.reset();
                limiter_meter.reset();
                auto_gain.clear_gains();
                playback_mode = None;
            }
            Err(error) => {
                eprintln!("realtime play process failed: {error:#}");
                reset_all(&mut renderers);
                limiter.reset();
                limiter_meter.reset();
                auto_gain.clear_gains();
                audio_output.finish();
                playback_mode = None;
            }
        }
    }
}

struct LiveMixControls<'a> {
    gains: &'a LiveGains,
    auto_gain: &'a AutoGainControl,
    sample_rate: f64,
    auto_gain_target_db: f32,
}

fn render_live_mix(
    renderers: &mut [RealtimeRenderer],
    instances: &mut [LiveInstanceState],
    clock_samples: &mut u64,
    controls: LiveMixControls<'_>,
    timeline: Option<&mut LiveTimelineState>,
    timing_window: &mut LiveTimingWindow,
) -> anyhow::Result<Vec<f32>> {
    let auto_gain_enabled = controls.auto_gain.enabled();
    let buf_size = renderers[0].buf_size() as u64;
    let chunk_start = *clock_samples;
    let timeline_sample_rate = SampleRate::new(controls.sample_rate)?;
    let block = BlockSpan::new(SamplePosition(chunk_start), buf_size as u32)?;
    let (scheduled, block_timing) = match timeline {
        Some(timeline) => {
            let scheduled = timeline
                .scheduler
                .take_block(block, LateEventPolicy::ClampToBlockStart);
            timing_window.observe_events(&scheduled);
            let timing = process_block_timing(block, timeline_sample_rate, &timeline.transport);
            (scheduled.events, timing)
        }
        None => (
            Vec::new(),
            process_block_timing(block, timeline_sample_rate, &FreeRunningTimeline),
        ),
    };
    let mut mixed = vec![0.0f32; buf_size as usize * 2];
    for (index, (renderer, instance)) in renderers.iter_mut().zip(instances.iter_mut()).enumerate()
    {
        if !instance.active {
            continue;
        }
        let mut events = take_chunk_events(&mut instance.queue, chunk_start, buf_size);
        events.extend(
            scheduled
                .iter()
                .filter(|event| usize::from(event.payload.instance_id) == index)
                .map(|event| LiveMidiEvent {
                    offset_frames: event.offset_frames,
                    message: event.payload.message,
                }),
        );
        events.sort_by_key(|event| event.offset_frames);
        match renderer.render_live_chunk_with_timing(&events, block_timing) {
            Ok(samples) => {
                let auto_gain = instance.auto_gain.process_block(
                    &samples,
                    controls.sample_rate,
                    controls.auto_gain_target_db,
                    auto_gain_enabled,
                );
                controls
                    .auto_gain
                    .set_gain_db(index, instance.auto_gain.gain_db());
                add_samples_ramped(
                    &mut mixed,
                    &samples,
                    auto_gain.scaled(controls.gains.get(index)),
                );
            }
            Err(error) => {
                eprintln!("realtime live instance {index} failed: {error:#}");
                renderer.reset();
                instance.active = false;
                instance.queue.clear();
                instance.auto_gain.reset();
                controls.auto_gain.set_gain_db(index, 0.0);
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
        timeline: None,
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

#[cfg(test)]
mod tests;
