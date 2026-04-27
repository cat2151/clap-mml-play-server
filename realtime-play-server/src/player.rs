use std::{
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};

use anyhow::{Context as _, Result};
use cmrt_core::{smf_render_stateless_with_options, CoreConfig, RenderOptions};
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};

pub(crate) trait PlayerHandle: Send + Sync + 'static {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()>;
    fn stop(&self) -> Result<()>;
}

pub(crate) struct PhaseAPlayer {
    inner: Arc<PlayerInner>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct PlayerInner {
    state: Mutex<PlayerState>,
    command_available: Condvar,
}

#[derive(Default)]
struct PlayerState {
    generation: u64,
    pending: Option<PlayerCommand>,
    active_sink: Option<Arc<Sink>>,
    shutdown: bool,
}

struct ActivePlayback {
    _stream: OutputStream,
    sink: Arc<Sink>,
}

enum PlayerCommand {
    Play { generation: u64, smf: Vec<u8> },
    Stop,
}

impl PhaseAPlayer {
    pub(crate) fn new(
        core_cfg: CoreConfig,
        plugin_path: String,
        render_options: RenderOptions,
    ) -> Result<Self> {
        let inner = Arc::new(PlayerInner::default());
        let worker_inner = Arc::clone(&inner);
        let (init_tx, init_rx) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("realtime-play-server-player".to_string())
            .spawn(move || {
                run_player_worker(worker_inner, core_cfg, plugin_path, render_options, init_tx);
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
            inner,
            worker: Mutex::new(Some(worker)),
        })
    }
}

impl PlayerHandle for PhaseAPlayer {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()> {
        self.inner.submit_play(smf)
    }

    fn stop(&self) -> Result<()> {
        self.inner.submit_stop()
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
    fn submit_play(&self, smf: Vec<u8>) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        let generation = state.generation;
        stop_active_locked(&mut state);
        state.pending = Some(PlayerCommand::Play { generation, smf });
        self.command_available.notify_one();
        Ok(())
    }

    fn submit_stop(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        stop_active_locked(&mut state);
        state.pending = Some(PlayerCommand::Stop);
        self.command_available.notify_one();
        Ok(())
    }

    fn take_command(&self) -> Option<PlayerCommand> {
        let mut state = self.state.lock().unwrap();
        while state.pending.is_none() && !state.shutdown {
            state = self.command_available.wait(state).unwrap();
        }
        if state.shutdown {
            return None;
        }
        state.pending.take()
    }

    fn generation_is_current(&self, generation: u64) -> bool {
        self.state.lock().unwrap().generation == generation
    }

    fn store_active_if_current(&self, generation: u64, active: &ActivePlayback) -> bool {
        let sink = Arc::clone(&active.sink);
        let mut state = self.state.lock().unwrap();
        if state.shutdown || state.generation != generation {
            active.sink.stop();
            return false;
        }
        state.active_sink = Some(sink);
        true
    }

    fn clear_active_if_current(&self, generation: u64) {
        let mut state = self.state.lock().unwrap();
        if state.generation == generation {
            state.active_sink = None;
        }
    }

    fn shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        state.shutdown = true;
        stop_active_locked(&mut state);
        self.command_available.notify_one();
    }
}

impl Drop for PhaseAPlayer {
    fn drop(&mut self) {
        self.inner.shutdown();
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

fn run_player_worker(
    inner: Arc<PlayerInner>,
    core_cfg: CoreConfig,
    plugin_path: String,
    render_options: RenderOptions,
    init_tx: std::sync::mpsc::Sender<std::result::Result<(), String>>,
) {
    let entry = match cmrt_core::load_entry(&plugin_path) {
        Ok(entry) => entry,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    let _ = init_tx.send(Ok(()));

    while let Some(command) = inner.take_command() {
        match command {
            PlayerCommand::Play { generation, smf } => {
                let samples = match smf_render_stateless_with_options(
                    &smf,
                    &core_cfg,
                    &entry,
                    render_options,
                ) {
                    Ok(samples) => samples,
                    Err(error) => {
                        eprintln!("realtime play render failed: {error:#}");
                        continue;
                    }
                };
                if !inner.generation_is_current(generation) {
                    continue;
                }
                let active = match start_rodio_playback(samples, core_cfg.sample_rate as u32) {
                    Ok(active) => active,
                    Err(error) => {
                        eprintln!("realtime play output failed: {error:#}");
                        continue;
                    }
                };
                if !inner.store_active_if_current(generation, &active) {
                    continue;
                }
                active.sink.sleep_until_end();
                inner.clear_active_if_current(generation);
            }
            PlayerCommand::Stop => {}
        }
    }
}

fn start_rodio_playback(samples: Vec<f32>, sample_rate: u32) -> Result<ActivePlayback> {
    let (stream, stream_handle) = OutputStream::try_default()
        .map_err(|e| anyhow::anyhow!("オーディオ出力の初期化失敗: {}", e))?;
    let sink =
        Sink::try_new(&stream_handle).map_err(|e| anyhow::anyhow!("Sink の作成失敗: {}", e))?;
    let source = SamplesBuffer::new(2, sample_rate, samples);
    sink.append(source);
    Ok(ActivePlayback {
        _stream: stream,
        sink: Arc::new(sink),
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

fn stop_active_locked(state: &mut PlayerState) {
    if let Some(sink) = state.active_sink.take() {
        sink.stop();
    }
}
