use super::*;

pub(super) struct CommandContext<'a> {
    /// bank ごとの worker。CLAP インスタンスも予備プールもこの向こう側にあり、
    /// ここからは触れない。
    pub(super) banks: &'a BankWorkers,
    pub(super) limiter: &'a mut MasterLimiter,
    pub(super) limiter_meter: &'a LimiterMeterState,
    pub(super) auto_gain: &'a AutoGainControl,
    pub(super) timing_window: &'a mut LiveTimingWindow,
    pub(super) timing_metrics: &'a TimingMetricsState,
    pub(super) audio_output: &'a AudioOutputControl,
    pub(super) playback_mode: &'a mut Option<PlaybackMode>,
    /// 進行中の先読みロード。**コマンド処理はここを空にしないまま帰ってよい。**
    /// 返事を引き取るのはレンダーループ（`standby::poll`）の仕事。
    pub(super) standby: &'a mut Option<StandbyLoad>,
}

pub(super) fn apply_command(context: CommandContext<'_>, command: PlayerCommand) {
    let CommandContext {
        banks,
        limiter,
        limiter_meter,
        auto_gain,
        timing_window,
        timing_metrics,
        audio_output,
        playback_mode,
        standby,
    } = context;
    if needs_idle_banks(&command) {
        // これから bank へ**同期の**仕事を出す。返事は 1 bank につき 1 本の channel に
        // 相乗りしているので、飛ばしたままの先読みを先に引き取らないと
        // 「どの要求の返事か」が入れ替わる。
        standby::settle(
            &mut standby_context(
                banks,
                limiter,
                limiter_meter,
                auto_gain,
                audio_output,
                playback_mode,
            ),
            standby,
        );
    }
    match command {
        PlayerCommand::Play {
            generation,
            schedule,
            patch,
        } => {
            banks.reset_all();
            limiter.reset();
            limiter_meter.reset();
            auto_gain.clear_gains();
            if let Err(error) = banks.prepare_scheduled_patch(patch.as_deref()) {
                eprintln!("realtime play patch load failed: {error}");
            }
            *playback_mode = Some(PlaybackMode::Scheduled {
                generation,
                playback: schedule,
            });
        }
        PlayerCommand::StopAll { generation } => {
            let _ = generation;
            eprintln!("cmrt-live: event=apply-stop-all");
            banks.release_all_notes();
            limiter.reset();
            limiter_meter.reset();
            auto_gain.clear_gains();
            *playback_mode = None;
        }
        PlayerCommand::StopInstance {
            generation,
            instance_id,
        } => {
            ensure_live_mode(playback_mode, generation, banks.instance_count());
            let instance_index = usize::from(instance_id);
            banks.release_instance_notes(instance_index);
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
                banks.release_all_notes();
                limiter.reset();
                limiter_meter.reset();
                auto_gain.clear_gains();
                *playback_mode = Some(new_live_mode(generation, banks.instance_count()));
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
            *timing_window = LiveTimingWindow::new(config.sample_rate_hz);
            timing_metrics.update(cmrt_realtime_ipc::TimingMetrics::default());
            match LiveTimelineState::new(config) {
                Ok(timeline) => {
                    let continued = begin_live_timeline(
                        playback_mode,
                        generation,
                        timeline,
                        banks.instance_count(),
                    );
                    if continued {
                        banks.release_all_notes_in_next_block();
                    } else {
                        banks.release_all_notes();
                        limiter.reset();
                        limiter_meter.reset();
                        auto_gain.clear_gains();
                    }
                }
                Err(error) => {
                    eprintln!("realtime live timeline failed: {error:#}");
                    banks.release_all_notes();
                    limiter.reset();
                    limiter_meter.reset();
                    auto_gain.clear_gains();
                    audio_output.finish();
                    *playback_mode = None;
                }
            }
        }
        PlayerCommand::FadeOutInstances {
            instance_ids,
            fade_frames,
        } => {
            if !matches!(playback_mode, Some(PlaybackMode::Live { .. })) {
                return;
            }
            eprintln!(
                "cmrt-live: event=apply-fade-out instances={instance_ids:?} frames={fade_frames}"
            );
            for instance_id in instance_ids {
                banks.fade_out_instance(usize::from(instance_id), fade_frames);
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
            effect_chain,
            completion,
        } => {
            ensure_live_mode(playback_mode, generation, banks.instance_count());
            let index = usize::from(instance_id);
            // **このスロットを書き換えた瞬間の再生位置。** クライアントが予約した
            // note on の位置（`at_frames`）と突き合わせるためだけにある。
            // 予約位置より後ろの clock で書き換えていたら、その note on が鳴る前に
            // 中身を差し替えてしまったということ＝別の小節が鳴る。
            let clock = match playback_mode.as_ref() {
                Some(PlaybackMode::Live { clock_samples, .. }) => *clock_samples,
                _ => 0,
            };
            eprintln!(
                "cmrt-live-patch: event=apply instance={index} clock={clock} patch={}",
                patch.as_deref().unwrap_or("-")
            );
            // 差し替えと settle は instance を所有している bank worker 上で走る。
            let result = banks.prepare_patch(index, patch.as_deref(), &effect_chain, true);
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
        PlayerCommand::PrepareStandbyLivePatch {
            generation,
            instance_id,
            patch,
            effect_chain,
            completion,
        } => {
            // **ここで待たない。** 送るだけで戻り、演奏 bank の render を続ける。
            // 完了を待ってクライアントへ返すのはレンダーループ（`standby::poll`）で、
            // それまで対象 bank は render 対象から外れる。
            standby::begin(
                &mut standby_context(
                    banks,
                    limiter,
                    limiter_meter,
                    auto_gain,
                    audio_output,
                    playback_mode,
                ),
                standby,
                standby::StandbyRequest {
                    generation,
                    instance_id,
                    patch,
                    effect_chain,
                    completion,
                },
            );
        }
        PlayerCommand::ProbeLivePatch {
            generation,
            instance_id,
            patch,
            completion,
        } => {
            ensure_live_mode(playback_mode, generation, banks.instance_count());
            let index = usize::from(instance_id);
            let result = banks.probe_patch(index, patch.as_deref());
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

/// 新しい timeline を live へ据える。前の演奏をそのまま続けて描いたら `true`。
///
/// 同じ generation の live が走っていれば、**sample clock・instance・auto gain を保ったまま**
/// timeline だけ差し替え、その 0 秒を今の clock に置く。前の演奏の note は呼び出し側が
/// NoteOff 済みで、その release が新しい timeline の頭の前に描かれる。未消化の生 MIDI は捨てる。
/// generation が変わっていれば（リングは既に捨てられている）live を作り直す。
pub(super) fn begin_live_timeline(
    playback_mode: &mut Option<PlaybackMode>,
    generation: u64,
    timeline: LiveTimelineState,
    instance_count: usize,
) -> bool {
    if let Some(PlaybackMode::Live {
        generation: live_generation,
        clock_samples,
        instances,
        timeline: slot,
    }) = playback_mode
    {
        if *live_generation == generation {
            for instance in instances.iter_mut() {
                instance.queue.clear();
            }
            *slot = Some(timeline.starting_at(*clock_samples));
            return true;
        }
    }
    let mut mode = new_live_mode(generation, instance_count);
    if let PlaybackMode::Live { timeline: slot, .. } = &mut mode {
        *slot = Some(timeline);
    }
    *playback_mode = Some(mode);
    false
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

/// bank へ同期の仕事を出すコマンドか。
///
/// この 3 つは `banks` へ送って**その場で返事を待つ**。先読みが飛んだままだと
/// 返事の対応がずれるので、処理前に先読みを畳む。live MIDI や停止はここに含めない
/// （含めると、鳴っている最中に先読みの完了待ちで演奏が止まる）。
fn needs_idle_banks(command: &PlayerCommand) -> bool {
    matches!(
        command,
        PlayerCommand::Play { .. }
            | PlayerCommand::PrepareLivePatch { .. }
            | PlayerCommand::ProbeLivePatch { .. }
    )
}

pub(super) fn ensure_live_mode(
    playback_mode: &mut Option<PlaybackMode>,
    generation: u64,
    instance_count: usize,
) {
    if !matches!(playback_mode, Some(PlaybackMode::Live { .. })) {
        *playback_mode = Some(new_live_mode(generation, instance_count));
    }
}
