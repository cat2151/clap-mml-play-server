mod audio_output;
mod commands;
mod limiter;
mod live;
mod output_stream;
mod runtime;
mod startup;
mod worker;

use std::{sync::Arc, sync::Mutex, thread::JoinHandle};

use self::audio_output::{new_audio_output, AudioOutputControl};
use self::commands::PlayerInner;
use self::live::{resolve_live_patch, validate_live_instance_id};
use self::runtime::{LimiterMeterState, LiveGains, LiveQueuedEvent};
use self::worker::{run_player_worker, WorkerOutput};
use anyhow::{Context as _, Result};
use cmrt_core::{smf_playback_schedule_with_options, CoreConfig, RenderOptions, VoicingReport};
use cmrt_realtime_ipc::{FastMidiEvent, InstanceId, LimiterMeter};

// ワーカースレッド（`worker`）が `super::` 経由で参照する。
use self::commands::PlayerCommand;

pub(crate) trait PlayerHandle: Send + Sync + 'static {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()>;
    fn play_mml(&self, mml: String) -> Result<()>;
    fn send_midi(&self, events: Vec<FastMidiEvent>) -> Result<()>;
    fn prepare_live_patch(&self, instance_id: InstanceId, patch: Option<String>) -> Result<()>;
    fn prepare_live_patch_with_voicing(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
    ) -> Result<VoicingReport>;
    fn set_live_buffer_multiplier(&self, multiplier: u8) -> Result<()>;
    /// live mix で instance へ掛ける振幅ゲイン（1.0 が等倍）。
    /// patch 差し替えで live を作り直しても保持される。
    fn set_live_instance_gain(&self, instance_id: InstanceId, gain: f32) -> Result<()>;
    fn stop_instance(&self, instance_id: InstanceId) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn limiter_meter(&self) -> LimiterMeter;
    fn underrun_frames(&self) -> u64;
}

pub(crate) struct RealtimePlayer {
    sample_rate: f64,
    render_options: RenderOptions,
    core_cfg: CoreConfig,
    inner: Arc<PlayerInner>,
    audio_output: Arc<AudioOutputControl>,
    limiter_meter: Arc<LimiterMeterState>,
    live_gains: Arc<LiveGains>,
    live_instance_count: usize,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl RealtimePlayer {
    pub(crate) fn new(
        core_cfg: CoreConfig,
        plugin_path: String,
        render_options: RenderOptions,
        live_instance_count: usize,
    ) -> Result<Self> {
        let sample_rate = core_cfg.sample_rate;
        let (audio_output, output_producer, output_consumer) =
            new_audio_output(core_cfg.buffer_size);
        let inner = Arc::new(PlayerInner::default());
        let limiter_meter = Arc::new(LimiterMeterState::default());
        let live_gains = Arc::new(LiveGains::default());
        let worker_inner = Arc::clone(&inner);
        let worker_audio_output = Arc::clone(&audio_output);
        let worker_limiter_meter = Arc::clone(&limiter_meter);
        let worker_live_gains = Arc::clone(&live_gains);
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
                        producer: output_producer,
                        consumer: output_consumer,
                    },
                    worker_core_cfg,
                    plugin_path,
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
            live_instance_count,
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

    fn prepare_live_patch(&self, instance_id: InstanceId, patch: Option<String>) -> Result<()> {
        self.validate_live_instance_id(instance_id)?;
        let patch = resolve_live_patch(patch, self.core_cfg.patches_dir.as_deref());
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

    fn prepare_live_patch_with_voicing(
        &self,
        instance_id: InstanceId,
        patch: Option<String>,
    ) -> Result<VoicingReport> {
        self.validate_live_instance_id(instance_id)?;
        let patch = resolve_live_patch(patch, self.core_cfg.patches_dir.as_deref());
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

    fn set_live_buffer_multiplier(&self, multiplier: u8) -> Result<()> {
        self.audio_output.set_buffer_multiplier(multiplier)
    }

    fn set_live_instance_gain(&self, instance_id: InstanceId, gain: f32) -> Result<()> {
        self.validate_live_instance_id(instance_id)?;
        self.live_gains.set(usize::from(instance_id), gain);
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
}

impl RealtimePlayer {
    fn validate_live_instance_id(&self, instance_id: InstanceId) -> Result<()> {
        validate_live_instance_id(instance_id, self.live_instance_count)
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
