mod audio_output;
mod output_stream;
mod worker;

use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};

use self::audio_output::{new_audio_output, AudioOutputControl};
use self::worker::run_player_worker;
use anyhow::{Context as _, Result};
use cmrt_core::{
    smf_playback_schedule_with_options, CoreConfig, RealtimePlaybackSchedule, RenderOptions,
    VoicingReport,
};

pub(crate) trait PlayerHandle: Send + Sync + 'static {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()>;
    fn play_mml(&self, mml: String) -> Result<()>;
    fn send_midi(&self, messages: Vec<[u8; 3]>, patch: Option<String>) -> Result<()>;
    fn prepare_live_patch(&self, patch: Option<String>) -> Result<()>;
    fn prepare_live_patch_with_voicing(&self, _patch: Option<String>) -> Result<VoicingReport> {
        anyhow::bail!("voicing probe is not supported by this player")
    }
    fn set_live_buffer_multiplier(&self, multiplier: u8) -> Result<()>;
    fn stop(&self) -> Result<()>;
}

pub(crate) struct RealtimePlayer {
    sample_rate: f64,
    render_options: RenderOptions,
    core_cfg: CoreConfig,
    inner: Arc<PlayerInner>,
    audio_output: Arc<AudioOutputControl>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct PlayerInner {
    state: Mutex<PlayerState>,
    command_available: Condvar,
}

#[derive(Default)]
struct PlayerState {
    generation: u64,
    pending: VecDeque<PlayerCommand>,
    live_requested: bool,
    shutdown: bool,
}

#[derive(Debug)]
enum PlayerCommand {
    Play {
        generation: u64,
        schedule: RealtimePlaybackSchedule,
        /// 再生前にロードするパッチ。None は初期音色（Init Saw）。
        patch: Option<String>,
    },
    Stop {
        generation: u64,
    },
    Midi {
        generation: u64,
        messages: Vec<[u8; 3]>,
        patch: Option<String>,
        enter_live: bool,
    },
    PrepareLivePatch {
        generation: u64,
        patch: Option<String>,
        completion: std::sync::mpsc::SyncSender<std::result::Result<(), String>>,
    },
    ProbeLivePatch {
        generation: u64,
        patch: Option<String>,
        completion: std::sync::mpsc::SyncSender<std::result::Result<VoicingReport, String>>,
    },
}

enum PlaybackMode {
    Scheduled {
        generation: u64,
        playback: RealtimePlaybackSchedule,
    },
    Live {
        generation: u64,
        pending_messages: Vec<[u8; 3]>,
    },
}

impl RealtimePlayer {
    pub(crate) fn new(
        core_cfg: CoreConfig,
        plugin_path: String,
        render_options: RenderOptions,
    ) -> Result<Self> {
        let sample_rate = core_cfg.sample_rate;
        let (audio_output, output_producer, output_consumer) =
            new_audio_output(core_cfg.buffer_size);
        let inner = Arc::new(PlayerInner::default());
        let worker_inner = Arc::clone(&inner);
        let worker_audio_output = Arc::clone(&audio_output);
        let worker_core_cfg = core_cfg.clone();
        let (init_tx, init_rx) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("realtime-play-server-player".to_string())
            .spawn(move || {
                run_player_worker(
                    worker_inner,
                    worker_audio_output,
                    worker_core_cfg,
                    plugin_path,
                    output_producer,
                    output_consumer,
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
            worker: Mutex::new(Some(worker)),
        })
    }
}

impl PlayerHandle for RealtimePlayer {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()> {
        let schedule =
            smf_playback_schedule_with_options(&smf, self.sample_rate, self.render_options)?;
        // SMF にはパッチ情報が無いため config のパッチ（起動時と同じ）を維持する
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

    fn send_midi(&self, messages: Vec<[u8; 3]>, patch: Option<String>) -> Result<()> {
        let patch = resolve_live_patch(patch, self.core_cfg.patches_dir.as_deref());
        self.inner
            .submit_midi(messages, patch, Arc::clone(&self.audio_output))
    }

    fn prepare_live_patch(&self, patch: Option<String>) -> Result<()> {
        let patch = resolve_live_patch(patch, self.core_cfg.patches_dir.as_deref());
        let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(0);
        self.inner.submit_prepare_live_patch(
            patch,
            completion_tx,
            Arc::clone(&self.audio_output),
        )?;
        completion_rx
            .recv()
            .context("realtime play worker exited while setting live patch")?
            .map_err(anyhow::Error::msg)
    }

    fn prepare_live_patch_with_voicing(&self, patch: Option<String>) -> Result<VoicingReport> {
        let patch = resolve_live_patch(patch, self.core_cfg.patches_dir.as_deref());
        let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(0);
        self.inner
            .submit_probe_live_patch(patch, completion_tx, Arc::clone(&self.audio_output))?;
        completion_rx
            .recv()
            .context("realtime play worker exited while probing live patch")?
            .map_err(anyhow::Error::msg)
    }

    fn set_live_buffer_multiplier(&self, multiplier: u8) -> Result<()> {
        self.audio_output.set_buffer_multiplier(multiplier)
    }

    fn stop(&self) -> Result<()> {
        self.inner.submit_stop(Arc::clone(&self.audio_output))
    }
}

impl Default for PlayerInner {
    fn default() -> Self {
        Self {
            state: Mutex::new(PlayerState::default()),
            command_available: Condvar::new(),
        }
    }
}

impl PlayerInner {
    fn submit_play(
        &self,
        schedule: RealtimePlaybackSchedule,
        patch: Option<String>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        let generation = state.generation;
        audio_output.start_generation(generation);
        state.live_requested = false;
        state.pending.clear();
        state.pending.push_back(PlayerCommand::Play {
            generation,
            schedule,
            patch,
        });
        self.command_available.notify_one();
        Ok(())
    }

    fn submit_stop(&self, audio_output: Arc<AudioOutputControl>) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        let generation = state.generation;
        audio_output.stop_generation(generation);
        state.live_requested = false;
        state.pending.clear();
        state.pending.push_back(PlayerCommand::Stop { generation });
        self.command_available.notify_one();
        Ok(())
    }

    fn submit_midi(
        &self,
        messages: Vec<[u8; 3]>,
        patch: Option<String>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        let enter_live = !state.live_requested;
        if enter_live {
            state.generation = next_generation(state.generation);
            // live MIDIは未処理の曲再生/停止requestより優先する。
            state.pending.clear();
            state.live_requested = true;
            audio_output.start_generation(state.generation);
        }
        let generation = state.generation;
        // 2回目以降も置換せずFIFOへ積む。これにより、別requestで届く複数の
        // Note On/Offを同じlive sessionの独立したvoiceとしてpluginへ渡せる。
        state.pending.push_back(PlayerCommand::Midi {
            generation,
            messages,
            patch,
            enter_live,
        });
        self.command_available.notify_one();
        Ok(())
    }

    fn submit_prepare_live_patch(
        &self,
        patch: Option<String>,
        completion: std::sync::mpsc::SyncSender<std::result::Result<(), String>>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        let generation = state.generation;
        audio_output.start_generation(generation);
        state.pending.clear();
        state.live_requested = true;
        state.pending.push_back(PlayerCommand::PrepareLivePatch {
            generation,
            patch,
            completion,
        });
        self.command_available.notify_one();
        Ok(())
    }

    fn submit_probe_live_patch(
        &self,
        patch: Option<String>,
        completion: std::sync::mpsc::SyncSender<std::result::Result<VoicingReport, String>>,
        audio_output: Arc<AudioOutputControl>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        let generation = state.generation;
        audio_output.start_generation(generation);
        state.pending.clear();
        state.live_requested = true;
        state.pending.push_back(PlayerCommand::ProbeLivePatch {
            generation,
            patch,
            completion,
        });
        self.command_available.notify_one();
        Ok(())
    }

    fn wait_for_command(&self) -> Option<PlayerCommand> {
        let mut state = self.state.lock().unwrap();
        while state.pending.is_empty() && !state.shutdown {
            state = self.command_available.wait(state).unwrap();
        }
        if state.shutdown {
            return None;
        }
        state.pending.pop_front()
    }

    fn pop_pending_command(&self) -> Option<PlayerCommand> {
        let mut state = self.state.lock().unwrap();
        if state.shutdown {
            return None;
        }
        state.pending.pop_front()
    }

    fn shutdown(&self, audio_output: &AudioOutputControl) {
        let mut state = self.state.lock().unwrap();
        state.shutdown = true;
        self.command_available.notify_one();
        audio_output.shutdown();
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

fn resolve_live_patch(patch: Option<String>, patches_dir: Option<&str>) -> Option<String> {
    patch
        .filter(|patch| !patch.trim().is_empty())
        .map(|patch| match patches_dir {
            Some(base) => Path::new(base).join(&patch).to_string_lossy().into_owned(),
            None => patch,
        })
}

fn ensure_running(state: &PlayerState) -> Result<()> {
    if state.shutdown {
        anyhow::bail!("realtime play worker is stopped");
    }
    Ok(())
}

fn next_generation(current: u64) -> u64 {
    current.wrapping_add(1).max(1)
}

#[cfg(test)]
#[path = "player/tests.rs"]
mod tests;
