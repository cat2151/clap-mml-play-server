use super::*;

const PATCH_SETTLE_BLOCKS: usize = 4;

pub(super) struct CommandContext<'a> {
    pub(super) renderers: &'a mut [RealtimeRenderer],
    pub(super) limiter: &'a mut MasterLimiter,
    pub(super) limiter_meter: &'a LimiterMeterState,
    pub(super) auto_gain: &'a AutoGainControl,
    pub(super) timing_window: &'a mut LiveTimingWindow,
    pub(super) timing_metrics: &'a TimingMetricsState,
    pub(super) audio_output: &'a AudioOutputControl,
    pub(super) playback_mode: &'a mut Option<PlaybackMode>,
}

pub(super) fn apply_command(context: CommandContext<'_>, command: PlayerCommand) {
    let CommandContext {
        renderers,
        limiter,
        limiter_meter,
        auto_gain,
        timing_window,
        timing_metrics,
        audio_output,
        playback_mode,
    } = context;
    match command {
        PlayerCommand::Play {
            generation,
            schedule,
            patch,
        } => {
            reset_all(renderers);
            limiter.reset();
            limiter_meter.reset();
            auto_gain.clear_gains();
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
            eprintln!("cmrt-live: event=apply-stop-all");
            reset_all(renderers);
            limiter.reset();
            limiter_meter.reset();
            auto_gain.clear_gains();
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
                auto_gain.set_gain_db(instance_index, 0.0);
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
                auto_gain.clear_gains();
                *playback_mode = Some(new_live_mode(generation, renderers.len()));
            }
            if let Some(PlaybackMode::Live {
                generation: live_generation,
                clock_samples,
                instances,
                timeline,
                ..
            }) = playback_mode
            {
                *live_generation = generation;
                eprintln!(
                    "cmrt-live: event=apply-midi enter_live={enter_live} count={} clock={} timeline={}",
                    events.len(),
                    *clock_samples,
                    timeline.is_some()
                );
                for event in events {
                    let instance = &mut instances[usize::from(event.instance_id)];
                    instance.active = true;
                    enqueue_live_event(&mut instance.queue, *clock_samples, event);
                }
            } else {
                // live モードに入れていない。生 MIDI はここで黙って捨てられる。
                eprintln!("cmrt-live: event=apply-midi-dropped enter_live={enter_live}");
            }
        }
        PlayerCommand::BeginLiveTimeline { generation, config } => {
            reset_all(renderers);
            limiter.reset();
            limiter_meter.reset();
            auto_gain.clear_gains();
            *timing_window = LiveTimingWindow::new(config.sample_rate_hz);
            timing_metrics.update(cmrt_realtime_ipc::TimingMetrics::default());
            match LiveTimelineState::new(config) {
                Ok(timeline) => {
                    let mut mode = new_live_mode(generation, renderers.len());
                    if let PlaybackMode::Live { timeline: slot, .. } = &mut mode {
                        *slot = Some(timeline);
                    }
                    *playback_mode = Some(mode);
                }
                Err(error) => {
                    eprintln!("realtime live timeline failed: {error:#}");
                    audio_output.finish();
                    *playback_mode = None;
                }
            }
        }
        PlayerCommand::SetLiveTempo { generation, change } => {
            apply_live_tempo(playback_mode, generation, change);
        }
        PlayerCommand::TimelineMidi { generation, events } => {
            let Some(PlaybackMode::Live {
                generation: live_generation,
                instances,
                timeline: Some(timeline),
                ..
            }) = playback_mode
            else {
                eprintln!("realtime timeline MIDI received without an active timeline");
                return;
            };
            *live_generation = generation;
            for event in events {
                let index = usize::from(event.instance_id);
                if timeline.scheduler.len() >= MAX_LIVE_QUEUE_EVENTS {
                    eprintln!(
                        "realtime timeline MIDI queue is full ({MAX_LIVE_QUEUE_EVENTS} events); dropping event"
                    );
                    continue;
                }
                instances[index].active = true;
                if let Err(error) = timeline.schedule(event) {
                    eprintln!("realtime timeline MIDI rejected: {error:#}");
                } else {
                    timeline.started = true;
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
                auto_gain.set_gain_db(index, 0.0);
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
                auto_gain.set_gain_db(index, 0.0);
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

/// 走っている timeline の tempo map へテンポ変化点を積む。
///
/// **`reset_all` も `new_live_mode` も呼ばないこと。** テンポ変更はタイムライン上の
/// データの追記であって、タイムラインの作り直しではない。呼ぶと変更のたびに
/// プラグインが初期化され、サンプルクロックの原点（`clock_samples`）も 0 へ戻る。
pub(super) fn apply_live_tempo(
    playback_mode: &mut Option<PlaybackMode>,
    generation: u64,
    change: cmrt_realtime_ipc::LiveTempoChange,
) {
    let Some(PlaybackMode::Live {
        generation: live_generation,
        timeline: Some(timeline),
        ..
    }) = playback_mode
    else {
        eprintln!("realtime live tempo received without an active timeline");
        return;
    };
    if timeline.id != change.timeline_id {
        // 作り直す前の timeline 宛。捨てる（今の演奏のテンポを動かさない）。
        return;
    }
    *live_generation = generation;
    if let Err(error) = timeline.set_tempo(change) {
        eprintln!("realtime live tempo rejected: {error:#}");
    }
}

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

pub(super) fn reset_all(renderers: &mut [RealtimeRenderer]) {
    for renderer in renderers {
        renderer.reset();
    }
}
