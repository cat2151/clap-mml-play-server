use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use cmrt_core::{CoreConfig, LiveMidiEvent};
use cpal::traits::StreamTrait;

use crate::timing;

use super::{
    audio_output::{AudioOutputConsumer, AudioOutputControl, AudioOutputProducer},
    auto_gain::target_rms_db,
    instances::PluginKind,
    limiter::MasterLimiter,
    live_capture::LiveCapture,
    mixer::add_samples_ramped,
    output_stream::build_output_stream,
    runtime::{
        new_live_instances, AutoGainControl, LiveGains, LiveInstanceState, LiveTimelineState,
        PlaybackMode, TimelinePayload, TimingMetricsState,
    },
    startup::create_live_renderers,
    timing_diagnostics::LiveTimingWindow,
    LimiterMeterState, LiveQueuedEvent, PlayerCommand, PlayerInner,
};

const OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(10);
const MAX_LIVE_QUEUE_EVENTS: usize = 8192;
mod bank;
mod command;
mod live_mix;
mod standby;

use self::bank::BankWorkers;
use self::live_mix::{render_live_mix, LiveMixControls};
use self::standby::{StandbyContext, StandbyLoad};
use command::{apply_command, CommandContext};
#[cfg(test)]
use command::{apply_live_tempo, begin_live_timeline};

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
    kinds: Vec<PluginKind>,
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
    let renderers = match create_live_renderers(&kinds[0], live_instance_count) {
        Ok(renderers) => renderers,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    timing::begin_startup_phase("audio_stream");
    // ここから先、CLAP インスタンスも予備プールも bank worker が所有する。この
    // coordinator は `process()` も `set_patch()` も直接呼ばない。
    // 予備プールはここから背景でインスタンスを作り始める。起動時のインスタンス生成が
    // 終わってから起こすことで、起動時間を取り合わない。
    let mut banks = BankWorkers::start(renderers, kinds, core_cfg.sample_rate);

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
    // 進行中の先読みロード。**持ったまま演奏 bank を回し続ける**。
    let mut standby: Option<StandbyLoad> = None;
    // live mix の出力そのものを録る診断用のタップ（`CMRT_LIVE_CAPTURE_WAV`）。
    // 既定では None なので、通常の演奏では 1 命令も増えない。
    let mut live_capture = LiveCapture::from_env(core_cfg.sample_rate);
    loop {
        // 先読みの返事を拾う。来ていなければ何もしないので、演奏は止まらない。
        standby::poll(
            &mut standby_context(
                &banks,
                &mut limiter,
                &limiter_meter,
                &auto_gain,
                &audio_output,
                &mut playback_mode,
            ),
            &mut standby,
        );
        let waiting_for_timeline_events = matches!(
            playback_mode,
            Some(PlaybackMode::Live {
                timeline: Some(ref timeline),
                ..
            }) if !timeline.started
        );
        if playback_mode.is_none() || waiting_for_timeline_events {
            // これから `wait_for_command()` で眠る。眠ると先読みの返事を誰も拾えなくなり、
            // クライアントが timeout まで返らない。render するものが無いこの経路でだけ待つ。
            standby::settle(
                &mut standby_context(
                    &banks,
                    &mut limiter,
                    &limiter_meter,
                    &auto_gain,
                    &audio_output,
                    &mut playback_mode,
                ),
                &mut standby,
            );
            let Some(command) = inner.wait_for_command() else {
                break;
            };
            apply_command(
                CommandContext {
                    banks: &banks,
                    limiter: &mut limiter,
                    limiter_meter: &limiter_meter,
                    auto_gain: &auto_gain,
                    timing_window: &mut timing_window,
                    timing_metrics: &timing_metrics,
                    audio_output: &audio_output,
                    playback_mode: &mut playback_mode,
                    standby: &mut standby,
                },
                command,
            );
            // 停止で演奏が終わったらここが書き出し点。プロセスを強制終了すると
            // worker 末尾までは走らないので、**止まった瞬間に書く**必要がある。
            if playback_mode.is_none() {
                if let Some(capture) = live_capture.as_mut() {
                    capture.finish_on_stop();
                }
            }
            continue;
        }
        if let Some(command) = inner.pop_pending_command() {
            apply_command(
                CommandContext {
                    banks: &banks,
                    limiter: &mut limiter,
                    limiter_meter: &limiter_meter,
                    auto_gain: &auto_gain,
                    timing_window: &mut timing_window,
                    timing_metrics: &timing_metrics,
                    audio_output: &audio_output,
                    playback_mode: &mut playback_mode,
                    standby: &mut standby,
                },
                command,
            );
            // 停止で演奏が終わったらここが書き出し点。プロセスを強制終了すると
            // worker 末尾までは走らないので、**止まった瞬間に書く**必要がある。
            if playback_mode.is_none() {
                if let Some(capture) = live_capture.as_mut() {
                    capture.finish_on_stop();
                }
            }
            continue;
        }
        if !output_producer.wait_for_space_timeout(OUTPUT_WAIT_TIMEOUT) {
            continue;
        }

        let render_started = Instant::now();
        let live_block = matches!(playback_mode, Some(PlaybackMode::Live { .. }));
        // render は clock を進めるので、ブロック先頭の位置は**呼ぶ前**に読む。
        let live_clock = match playback_mode.as_ref() {
            Some(PlaybackMode::Live { clock_samples, .. }) => *clock_samples,
            _ => 0,
        };
        let render_result = match playback_mode.as_mut() {
            Some(PlaybackMode::Scheduled {
                generation,
                playback,
            }) => banks
                .render_scheduled(playback)
                .map(|chunk| chunk.map(|samples| (*generation, samples))),
            Some(PlaybackMode::Live {
                generation,
                clock_samples,
                instances,
                timeline,
            }) => render_live_mix(
                &banks,
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
                if live_block {
                    if let Some(capture) = live_capture.as_mut() {
                        capture.push(&samples, live_clock);
                    }
                }
                if !output_producer.push_chunk(generation, samples) {
                    playback_mode = None;
                } else if live_block {
                    let block_duration =
                        Duration::from_secs_f64(banks.buf_size() as f64 / core_cfg.sample_rate);
                    timing_window.observe_block(
                        render_elapsed,
                        block_duration,
                        output_producer.lead_frames() as u64,
                    );
                    timing_window.publish_if_due(&timing_metrics, Instant::now());
                }
            }
            Ok(None) => {
                if let Some(capture) = live_capture.as_mut() {
                    capture.finish();
                }
                audio_output.finish();
                limiter.reset();
                limiter_meter.reset();
                auto_gain.clear_gains();
                playback_mode = None;
            }
            Err(error) => {
                eprintln!("realtime play process failed: {error:#}");
                if let Some(capture) = live_capture.as_mut() {
                    capture.finish();
                }
                banks.reset_all();
                limiter.reset();
                limiter_meter.reset();
                auto_gain.clear_gains();
                audio_output.finish();
                playback_mode = None;
            }
        }
    }
    // 録っていれば書き出す。停止コマンドで抜けた経路はここが唯一の書き出し点。
    if let Some(capture) = live_capture.as_mut() {
        capture.finish();
    }
    // 待っているクライアントを必ず解放してから畳む。ここで拾わないと、先読みの
    // 完了待ちが timeout まで返らない。
    standby::settle(
        &mut standby_context(
            &banks,
            &mut limiter,
            &limiter_meter,
            &auto_gain,
            &audio_output,
            &mut playback_mode,
        ),
        &mut standby,
    );
    // 両 bank へ Shutdown を送って join する。ここを通らずに落ちても
    // `BankWorkers` の Drop が同じことをする。
    banks.shutdown();
}

/// 先読みの進行に要る持ち物をまとめ直す。レンダーループの局所変数から作る。
fn standby_context<'a>(
    banks: &'a BankWorkers,
    limiter: &'a mut MasterLimiter,
    limiter_meter: &'a LimiterMeterState,
    auto_gain: &'a AutoGainControl,
    audio_output: &'a AudioOutputControl,
    playback_mode: &'a mut Option<PlaybackMode>,
) -> StandbyContext<'a> {
    StandbyContext {
        banks,
        limiter,
        limiter_meter,
        auto_gain,
        audio_output,
        playback_mode,
    }
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
