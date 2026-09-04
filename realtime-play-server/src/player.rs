mod audio_output;
mod auto_gain;
mod bank;
mod commands;
mod instances;
mod limiter;
mod live;
mod live_capture;
mod mixer;
mod output_stream;
mod runtime;
mod startup;
mod timing_diagnostics;
mod worker;

use std::{
    sync::mpsc::{Receiver, SyncSender, TryRecvError},
    sync::Arc,
    sync::Mutex,
    thread::JoinHandle,
};

use self::audio_output::{new_audio_output, AudioOutputControl};
use self::bank::BankLayout;
use self::commands::PlayerInner;
use self::instances::PatchBases;
use self::live::{resolve_live_patch, validate_live_instance_id};
use self::runtime::{
    AutoGainControl, LimiterMeterState, LiveGains, LiveQueuedEvent, TimingMetricsState,
};
use self::worker::{run_player_worker, WorkerOutput};
use anyhow::{Context as _, Result};
use cmrt_core::{smf_playback_schedule_with_options, CoreConfig, RenderOptions, VoicingReport};
use cmrt_realtime_ipc::{
    FastMidiEvent, InstanceId, LimiterMeter, LiveTempoChange, LiveTimelineConfig,
    TimelineMidiEvent, TimingMetrics,
};

// ワーカースレッド（`worker`）が `super::` 経由で参照する。
use self::commands::PlayerCommand;

pub(crate) use self::instances::{plugin_kinds, PluginKind};

/// 先読みロードの結果。ワーカー間は `String` で運ぶ（`anyhow::Error` は Send 境界を
/// 跨がせたくないため、既存の patch load 系と同じ形に揃えてある）。
pub(crate) type StandbyLoadResult = std::result::Result<(), String>;

/// 先読みロードの受付票。
///
/// [`PlayerHandle::begin_standby_live_patch`] が返す。ロードは対象 bank の worker
/// 上で走り続けていて、この受付票を持っているスレッド（fast IPC 受信スレッド）は
/// **待たずに他のコマンドを捌く**。
///
/// 完了送信路は容量 1 なので、受け取り手が poll していなくても coordinator 側の
/// `send` が block しない。受付票を drop してもロードは止まらない。
pub(crate) struct StandbyLoadTicket {
    completion: Receiver<StandbyLoadResult>,
}

/// 受付票と、その完了を送る側の組を作る。
///
/// 容量 1 の同期チャネルであることがこの設計の要。0（rendezvous）にすると
/// coordinator の `send` が受け取り手を待って止まり、レンダーループごと固まる。
pub(crate) fn standby_completion_channel() -> (SyncSender<StandbyLoadResult>, StandbyLoadTicket) {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    (tx, StandbyLoadTicket { completion: rx })
}

impl StandbyLoadTicket {
    /// 完了していれば結果を返す。**まだなら `None`。決して block しない。**
    ///
    /// 送信側が結果を送らずに消えた場合（ワーカー停止）も `Some(Err(_))` を返す。
    /// ここで `None` を返し続けると、クライアントが永久に「ロード中」のまま残る。
    pub(crate) fn poll(&self) -> Option<Result<()>> {
        match self.completion.try_recv() {
            Ok(result) => Some(result.map_err(anyhow::Error::msg)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                "realtime play worker exited while preloading a standby patch"
            ))),
        }
    }
}

pub(crate) trait PlayerHandle: Send + Sync + 'static {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()>;
    fn play_mml(&self, mml: String) -> Result<()>;
    fn send_midi(&self, events: Vec<FastMidiEvent>) -> Result<()>;
    fn begin_live_timeline(&self, config: LiveTimelineConfig) -> Result<()>;
    /// 走っている live timeline の tempo map へテンポ変化点を積む。
    /// `begin_live_timeline` と違い、timeline もプラグインの状態も作り直さない。
    fn set_live_tempo(&self, change: LiveTempoChange) -> Result<()>;
    fn send_timeline_midi(&self, events: Vec<TimelineMidiEvent>) -> Result<()>;
    fn prepare_live_patch(&self, instance_id: InstanceId, patch: Option<String>) -> Result<()>;
    /// 非演奏 bank への先読みロードを **受け付けるだけ**。
    ///
    /// [`PlayerHandle::prepare_live_patch`] と違い、クライアントが「この instance は
    /// 鳴っている bank に属さない」と宣言している。サーバーはこれを根拠に、その bank の
    /// レンダーを止めてロードしてよい。
    ///
    /// **戻り値はロードの完了ではなく受付票**（[`StandbyLoadTicket`]）である。
    /// 重い音色は数秒かかるので、ここで待つと IPC 受信スレッドが塞がり、演奏中の
    /// bank 宛 timeline MIDI がロード終了まで一切届かなくなる（16 分音符が
    /// 伸び切って聞こえた実障害の原因）。完了は受付票を非 blocking に
    /// [`StandbyLoadTicket::poll`] して拾うこと。
    fn begin_standby_live_patch(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
    ) -> Result<StandbyLoadTicket>;
    fn prepare_live_patch_with_voicing(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
    ) -> Result<VoicingReport>;
    fn set_live_buffer_multiplier(&self, multiplier: u16) -> Result<()>;
    /// live mix で instance へ掛ける振幅ゲイン（1.0 が等倍）。
    /// patch 差し替えで live を作り直しても保持される。
    fn set_live_instance_gain(&self, instance_id: InstanceId, gain: f32) -> Result<()>;
    /// live mixのinstance別RMS auto-trimを切り替える。
    fn set_live_auto_gain_enabled(&self, enabled: bool) -> Result<()>;
    fn stop_instance(&self, instance_id: InstanceId) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn limiter_meter(&self) -> LimiterMeter;
    fn underrun_frames(&self) -> u64;
    fn timing_metrics(&self) -> TimingMetrics;
    /// live instance ごとに auto-trim が掛けているゲイン（dB）。auto gain が
    /// off か、まだ何も鳴っていない instance は 0 dB。
    fn auto_gain_db(&self) -> Vec<f32>;
}

pub(crate) struct RealtimePlayer {
    sample_rate: f64,
    render_options: RenderOptions,
    core_cfg: CoreConfig,
    inner: Arc<PlayerInner>,
    audio_output: Arc<AudioOutputControl>,
    limiter_meter: Arc<LimiterMeterState>,
    live_gains: Arc<LiveGains>,
    auto_gain: Arc<AutoGainControl>,
    timing_metrics: Arc<TimingMetricsState>,
    live_instance_count: usize,
    /// 音色の相対パスを絶対パスへ直す基点。プラグインごとに音色置き場が違うので形ごとに持つ。
    patch_bases: PatchBases,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl RealtimePlayer {
    pub(crate) fn new(
        core_cfg: CoreConfig,
        kinds: Vec<PluginKind>,
        render_options: RenderOptions,
        live_instance_count: usize,
    ) -> Result<Self> {
        let sample_rate = core_cfg.sample_rate;
        let patch_bases = PatchBases::from_kinds(&kinds);
        let (audio_output, output_producer, output_consumer) =
            new_audio_output(core_cfg.buffer_size);
        let inner = Arc::new(PlayerInner::default());
        let limiter_meter = Arc::new(LimiterMeterState::default());
        let live_gains = Arc::new(LiveGains::default());
        let auto_gain = Arc::new(AutoGainControl::default());
        let timing_metrics = Arc::new(TimingMetricsState::default());
        let worker_inner = Arc::clone(&inner);
        let worker_audio_output = Arc::clone(&audio_output);
        let worker_limiter_meter = Arc::clone(&limiter_meter);
        let worker_live_gains = Arc::clone(&live_gains);
        let worker_auto_gain = Arc::clone(&auto_gain);
        let worker_timing_metrics = Arc::clone(&timing_metrics);
        let worker_core_cfg = core_cfg.clone();
        let (init_tx, init_rx) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("realtime-play-server-player".to_string())
            .spawn(move || {
                run_player_worker(
                    worker_inner,
                    WorkerOutput {
                        control: worker_audio_output,
                        limiter_meter: worker_limiter_meter,
                        live_gains: worker_live_gains,
                        auto_gain: worker_auto_gain,
                        timing_metrics: worker_timing_metrics,
                        producer: output_producer,
                        consumer: output_consumer,
                    },
                    worker_core_cfg,
                    kinds,
                    live_instance_count,
                    init_tx,
                );
            })
            .context("failed to spawn realtime play worker")?;

        let init_result = init_rx
            .recv()
            .context("realtime play worker exited before initialization")?;
        if let Err(message) = init_result {
            let _ = worker.join();
            anyhow::bail!(message);
        }

        Ok(Self {
            sample_rate,
            render_options,
            core_cfg,
            inner,
            audio_output,
            limiter_meter,
            live_gains,
            auto_gain,
            timing_metrics,
            live_instance_count,
            patch_bases,
            worker: Mutex::new(Some(worker)),
        })
    }
}

impl PlayerHandle for RealtimePlayer {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()> {
        let schedule =
            smf_playback_schedule_with_options(&smf, self.sample_rate, self.render_options)?;
        self.inner.submit_play(
            schedule,
            self.core_cfg.patch_path.clone(),
            Arc::clone(&self.audio_output),
        )
    }

    fn play_mml(&self, mml: String) -> Result<()> {
        let prepared = cmrt_core::prepare_realtime_play(&mml, &self.core_cfg)?;
        let schedule = smf_playback_schedule_with_options(
            &prepared.smf_bytes,
            self.sample_rate,
            self.render_options,
        )?;
        self.inner.submit_play(
            schedule,
            prepared.patch_path,
            Arc::clone(&self.audio_output),
        )
    }

    fn send_midi(&self, events: Vec<FastMidiEvent>) -> Result<()> {
        if events.is_empty() {
            anyhow::bail!("MIDI events must not be empty");
        }
        for event in &events {
            self.validate_live_instance_id(event.instance_id)?;
        }
        self.inner
            .submit_midi(events, Arc::clone(&self.audio_output))
    }

    fn begin_live_timeline(&self, config: LiveTimelineConfig) -> Result<()> {
        if (config.sample_rate_hz - self.sample_rate).abs() > f64::EPSILON * self.sample_rate {
            anyhow::bail!(
                "timeline sample rate {} does not match server {}",
                config.sample_rate_hz,
                self.sample_rate
            );
        }
        self.inner
            .submit_begin_live_timeline(config, Arc::clone(&self.audio_output))
    }

    fn set_live_tempo(&self, change: LiveTempoChange) -> Result<()> {
        self.inner.submit_set_live_tempo(change)
    }

    fn send_timeline_midi(&self, events: Vec<TimelineMidiEvent>) -> Result<()> {
        if events.is_empty() {
            anyhow::bail!("timeline MIDI events must not be empty");
        }
        for event in &events {
            self.validate_live_instance_id(event.instance_id)?;
        }
        self.inner.submit_timeline_midi(events)
    }

    fn prepare_live_patch(&self, instance_id: InstanceId, patch: Option<String>) -> Result<()> {
        self.validate_live_instance_id(instance_id)?;
        let patch = resolve_live_patch(patch, &self.patch_bases);
        let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(0);
        self.inner.submit_prepare_live_patch(
            instance_id,
            patch,
            completion_tx,
            Arc::clone(&self.audio_output),
        )?;
        completion_rx
            .recv()
            .context("realtime play worker exited while setting live patch")?
            .map_err(anyhow::Error::msg)
    }

    /// 非演奏 bank への先読みを受け付ける。**完了は待たない。**
    ///
    /// ロードそのものは、対象 instance を所有する bank worker の上で走る
    /// （`player/worker/bank.rs`）。coordinator は専用コマンド
    /// [`PlayerCommand::PrepareStandbyLivePatch`] を受けた時点でその bank を
    /// render-disabled にし、ロードの完了を待たずに演奏 bank を回し続ける。
    /// **ここ（IPC 受信スレッド）も待たない。** 待つのをやめたのが v10 の要点で、
    /// 待っていた頃はロード中の timeline MIDI が一切 dispatch されなかった。
    ///
    /// **ここが出す `thread=` は IPC 受信スレッドで、ロードした thread ではない。**
    /// どのスレッドがロードしたかは bank worker が出す `cmrt-bank-patch:` を見ること。
    /// この行は「どの bank の要求として受けたか」を確かめるためのもの。
    /// wire の request ID と `accepted` / `completed` の対応は `fast_ipc.rs` が出す
    /// 同じ `cmrt-standby-patch:` 行（`request=` 付き）を見ること。
    fn begin_standby_live_patch(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
    ) -> Result<StandbyLoadTicket> {
        let slot = self.bank_layout()?.slot_of(instance_id)?;
        let result = self.submit_standby_live_patch(instance_id, patch);
        let event = if result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        eprintln!(
            "cmrt-standby-patch: bank={} local={} instance={instance_id} thread={:?} event={event}",
            slot.bank,
            slot.local_index,
            std::thread::current().id(),
        );
        result
    }

    fn prepare_live_patch_with_voicing(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
    ) -> Result<VoicingReport> {
        self.validate_live_instance_id(instance_id)?;
        let patch = resolve_live_patch(patch, &self.patch_bases);
        let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(0);
        self.inner.submit_probe_live_patch(
            instance_id,
            patch,
            completion_tx,
            Arc::clone(&self.audio_output),
        )?;
        completion_rx
            .recv()
            .context("realtime play worker exited while probing live patch")?
            .map_err(anyhow::Error::msg)
    }

    fn set_live_buffer_multiplier(&self, multiplier: u16) -> Result<()> {
        self.audio_output.set_buffer_multiplier(multiplier)
    }

    fn set_live_instance_gain(&self, instance_id: InstanceId, gain: f32) -> Result<()> {
        self.validate_live_instance_id(instance_id)?;
        self.live_gains.set(usize::from(instance_id), gain);
        Ok(())
    }

    fn set_live_auto_gain_enabled(&self, enabled: bool) -> Result<()> {
        self.auto_gain.set_enabled(enabled);
        Ok(())
    }

    fn stop_instance(&self, instance_id: InstanceId) -> Result<()> {
        self.validate_live_instance_id(instance_id)?;
        self.inner
            .submit_stop_instance(instance_id, Arc::clone(&self.audio_output))
    }

    fn stop(&self) -> Result<()> {
        self.inner.submit_stop(Arc::clone(&self.audio_output))
    }

    fn limiter_meter(&self) -> LimiterMeter {
        self.limiter_meter.snapshot()
    }

    fn underrun_frames(&self) -> u64 {
        self.audio_output.underrun_frames()
    }

    fn timing_metrics(&self) -> TimingMetrics {
        self.timing_metrics.snapshot()
    }

    fn auto_gain_db(&self) -> Vec<f32> {
        self.auto_gain.gains_db()
    }
}

impl RealtimePlayer {
    fn validate_live_instance_id(&self, instance_id: InstanceId) -> Result<()> {
        validate_live_instance_id(instance_id, self.live_instance_count)
    }

    /// 先読みコマンドを積んで、受付票だけを返す。**誰も待たない。**
    ///
    /// coordinator も IPC 受信スレッドもロード完了を待たず、完了は容量 1 の
    /// チャネル越しに受付票へ落ちる。
    fn submit_standby_live_patch(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
    ) -> Result<StandbyLoadTicket> {
        let patch = resolve_live_patch(patch, &self.patch_bases);
        let (completion_tx, ticket) = standby_completion_channel();
        self.inner.submit_prepare_standby_live_patch(
            instance_id,
            patch,
            completion_tx,
            Arc::clone(&self.audio_output),
        )?;
        Ok(ticket)
    }

    /// 設定された live instance 数を 2 bank へ割る規則。
    ///
    /// 割り切れない設定（`CMRT_LIVE_INSTANCE_COUNT=1`）では先読みが成り立たないので、
    /// [`PlayerHandle::prepare_standby_live_patch`] だけがここで失敗する。通常の
    /// patch load はこの制約と無関係に動き続ける。
    fn bank_layout(&self) -> Result<BankLayout> {
        BankLayout::new(self.live_instance_count)
    }
}

impl Drop for RealtimePlayer {
    fn drop(&mut self) {
        self.inner.shutdown(&self.audio_output);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests;
