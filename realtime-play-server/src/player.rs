use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Context as _, Result};
use cmrt_core::{
    smf_playback_schedule_with_options, CoreConfig, RealtimePlaybackSchedule, RealtimeRenderer,
    RenderOptions,
};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
};

pub(crate) trait PlayerHandle: Send + Sync + 'static {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()>;
    fn stop(&self) -> Result<()>;
}

const MAX_BUFFERED_CHUNKS: usize = 8;
const OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(10);

pub(crate) struct RealtimePlayer {
    sample_rate: f64,
    render_options: RenderOptions,
    inner: Arc<PlayerInner>,
    audio_output: Arc<AudioOutputBuffer>,
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
    shutdown: bool,
}

#[derive(Default)]
struct AudioOutputBuffer {
    state: Mutex<AudioOutputState>,
    state_changed: Condvar,
}

#[derive(Debug)]
enum PlayerCommand {
    Play {
        generation: u64,
        schedule: RealtimePlaybackSchedule,
    },
    Stop {
        generation: u64,
    },
}

#[derive(Default)]
struct AudioOutputState {
    generation: u64,
    chunks: VecDeque<AudioChunk>,
    shutdown: bool,
}

struct AudioChunk {
    samples: Vec<f32>,
    offset: usize,
}

impl RealtimePlayer {
    pub(crate) fn new(
        core_cfg: CoreConfig,
        plugin_path: String,
        render_options: RenderOptions,
    ) -> Result<Self> {
        let sample_rate = core_cfg.sample_rate;
        let audio_output = Arc::new(AudioOutputBuffer::default());
        let inner = Arc::new(PlayerInner::default());
        let worker_inner = Arc::clone(&inner);
        let worker_audio_output = Arc::clone(&audio_output);
        let (init_tx, init_rx) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("realtime-play-server-player".to_string())
            .spawn(move || {
                run_player_worker(
                    worker_inner,
                    worker_audio_output,
                    core_cfg,
                    plugin_path,
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
        self.inner
            .submit_play(schedule, Arc::clone(&self.audio_output))
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
        audio_output: Arc<AudioOutputBuffer>,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        let generation = state.generation;
        audio_output.reset(generation);
        state.pending = Some(PlayerCommand::Play {
            generation,
            schedule,
        });
        self.command_available.notify_one();
        Ok(())
    }

    fn submit_stop(&self, audio_output: Arc<AudioOutputBuffer>) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        ensure_running(&state)?;
        state.generation = next_generation(state.generation);
        let generation = state.generation;
        audio_output.reset(generation);
        state.pending = Some(PlayerCommand::Stop { generation });
        self.command_available.notify_one();
        Ok(())
    }

    fn wait_for_command(&self) -> Option<PlayerCommand> {
        let mut state = self.state.lock().unwrap();
        while state.pending.is_none() && !state.shutdown {
            state = self.command_available.wait(state).unwrap();
        }
        if state.shutdown {
            return None;
        }
        state.pending.take()
    }

    fn pop_pending_command(&self) -> Option<PlayerCommand> {
        let mut state = self.state.lock().unwrap();
        if state.shutdown {
            return None;
        }
        state.pending.take()
    }

    fn shutdown(&self, audio_output: &AudioOutputBuffer) {
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

fn run_player_worker(
    inner: Arc<PlayerInner>,
    audio_output: Arc<AudioOutputBuffer>,
    core_cfg: CoreConfig,
    plugin_path: String,
    init_tx: std::sync::mpsc::Sender<std::result::Result<(), String>>,
) {
    let entry = match cmrt_core::load_entry(&plugin_path) {
        Ok(entry) => entry,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    let mut renderer = match RealtimeRenderer::new(&core_cfg, &entry) {
        Ok(renderer) => renderer,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    let output_stream = match build_output_stream(&audio_output, core_cfg.sample_rate) {
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
    let _ = init_tx.send(Ok(()));

    let _keep_stream_alive = output_stream;
    let mut current_playback: Option<(u64, RealtimePlaybackSchedule)> = None;
    loop {
        if current_playback.is_none() {
            let Some(command) = inner.wait_for_command() else {
                break;
            };
            apply_command(&mut renderer, &mut current_playback, command);
            continue;
        }

        if let Some(command) = inner.pop_pending_command() {
            apply_command(&mut renderer, &mut current_playback, command);
            continue;
        }

        if !audio_output.wait_for_space_timeout(OUTPUT_WAIT_TIMEOUT) {
            continue;
        }

        let Some((generation, playback)) = current_playback.as_mut() else {
            continue;
        };
        match renderer.render_next_chunk(playback) {
            Ok(Some(chunk)) => {
                if !audio_output.push_chunk(*generation, chunk) {
                    current_playback = None;
                }
            }
            Ok(None) => current_playback = None,
            Err(error) => {
                eprintln!("realtime play process failed: {error:#}");
                renderer.reset();
                current_playback = None;
            }
        }
    }
}

fn apply_command(
    renderer: &mut RealtimeRenderer,
    current_playback: &mut Option<(u64, RealtimePlaybackSchedule)>,
    command: PlayerCommand,
) {
    renderer.reset();
    match command {
        PlayerCommand::Play {
            generation,
            schedule,
        } => *current_playback = Some((generation, schedule)),
        PlayerCommand::Stop { generation } => {
            let _ = generation;
            *current_playback = None;
        }
    }
}

fn build_output_stream(audio_output: &Arc<AudioOutputBuffer>, sample_rate: f64) -> Result<Stream> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("既定のオーディオ出力デバイスが見つかりません"))?;
    let default_config = device
        .default_output_config()
        .map_err(|e| anyhow::anyhow!("既定の出力設定が取得できません: {}", e))?;
    let device_sample_rate = default_config.sample_rate().0;
    if device_sample_rate != sample_rate as u32 {
        anyhow::bail!(
            "既定の出力デバイス sample rate ({device_sample_rate}) が config.toml の sample_rate ({}) と一致しません",
            sample_rate as u32
        );
    }

    let stream_config = StreamConfig {
        channels: default_config.channels(),
        sample_rate: default_config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    let err_fn = |error| eprintln!("realtime play output stream error: {error}");
    match default_config.sample_format() {
        SampleFormat::F32 => build_typed_output_stream::<f32>(
            &device,
            &stream_config,
            Arc::clone(audio_output),
            err_fn,
        ),
        SampleFormat::I16 => build_typed_output_stream::<i16>(
            &device,
            &stream_config,
            Arc::clone(audio_output),
            err_fn,
        ),
        SampleFormat::U16 => build_typed_output_stream::<u16>(
            &device,
            &stream_config,
            Arc::clone(audio_output),
            err_fn,
        ),
        other => anyhow::bail!("未対応の出力サンプル形式です: {other:?}"),
    }
}

fn build_typed_output_stream<T>(
    device: &cpal::Device,
    stream_config: &StreamConfig,
    audio_output: Arc<AudioOutputBuffer>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream>
where
    T: Sample + FromSample<f32> + SizedSample,
{
    let channels = stream_config.channels as usize;
    device
        .build_output_stream(
            stream_config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                audio_output.fill_output(data, channels);
            },
            err_fn,
            None,
        )
        .map_err(|e| anyhow::anyhow!("オーディオ出力 stream の作成失敗: {}", e))
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

impl AudioOutputBuffer {
    fn reset(&self, generation: u64) {
        let mut state = self.state.lock().unwrap();
        state.generation = generation;
        state.chunks.clear();
        self.state_changed.notify_all();
    }

    fn shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        state.shutdown = true;
        state.chunks.clear();
        self.state_changed.notify_all();
    }

    fn wait_for_space_timeout(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.shutdown || state.chunks.len() < MAX_BUFFERED_CHUNKS {
            return !state.shutdown;
        }
        let (state_after_wait, _) = self.state_changed.wait_timeout(state, timeout).unwrap();
        state = state_after_wait;
        !state.shutdown && state.chunks.len() < MAX_BUFFERED_CHUNKS
    }

    fn push_chunk(&self, generation: u64, samples: Vec<f32>) -> bool {
        let mut state = self.state.lock().unwrap();
        while !state.shutdown
            && state.generation == generation
            && state.chunks.len() >= MAX_BUFFERED_CHUNKS
        {
            state = self.state_changed.wait(state).unwrap();
        }
        if state.shutdown || state.generation != generation {
            return false;
        }
        state.chunks.push_back(AudioChunk { samples, offset: 0 });
        true
    }

    fn fill_output<T>(&self, output: &mut [T], channels: usize)
    where
        T: Sample + FromSample<f32>,
    {
        if channels == 0 {
            return;
        }

        let Ok(mut state) = self.state.try_lock() else {
            zero_output(output);
            return;
        };
        let mut consumed = false;
        for frame in output.chunks_mut(channels) {
            let (left, right) = next_stereo_sample(&mut state).unwrap_or((0.0, 0.0));
            consumed = true;
            if channels == 1 {
                frame[0] = T::from_sample((left + right) * 0.5);
                continue;
            }
            frame[0] = T::from_sample(left);
            frame[1] = T::from_sample(right);
            for sample in &mut frame[2..] {
                *sample = T::from_sample(0.0);
            }
        }
        if consumed {
            self.state_changed.notify_all();
        }
    }
}

fn next_stereo_sample(state: &mut AudioOutputState) -> Option<(f32, f32)> {
    loop {
        let chunk = state.chunks.front_mut()?;
        if chunk.offset + 1 >= chunk.samples.len() {
            state.chunks.pop_front();
            continue;
        }
        let left = chunk.samples[chunk.offset];
        let right = chunk.samples[chunk.offset + 1];
        chunk.offset += 2;
        if chunk.offset >= chunk.samples.len() {
            state.chunks.pop_front();
        }
        return Some((left, right));
    }
}

fn zero_output<T>(output: &mut [T])
where
    T: Sample + FromSample<f32>,
{
    for sample in output {
        *sample = T::from_sample(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_play_replaces_older_pending_play() {
        let inner = PlayerInner::default();
        let audio_output = Arc::new(AudioOutputBuffer::default());

        inner
            .submit_play(
                RealtimePlaybackSchedule::new(vec![], 10),
                Arc::clone(&audio_output),
            )
            .unwrap();
        inner
            .submit_play(
                RealtimePlaybackSchedule::new(vec![], 20),
                Arc::clone(&audio_output),
            )
            .unwrap();

        match inner.wait_for_command() {
            Some(PlayerCommand::Play {
                generation,
                schedule,
            }) => {
                assert_eq!(generation, 2);
                assert_eq!(schedule.total_samples(), 20);
            }
            other => panic!("expected latest play command, got {other:?}"),
        }
    }

    #[test]
    fn submit_stop_replaces_pending_play() {
        let inner = PlayerInner::default();
        let audio_output = Arc::new(AudioOutputBuffer::default());

        inner
            .submit_play(
                RealtimePlaybackSchedule::new(vec![], 10),
                Arc::clone(&audio_output),
            )
            .unwrap();
        inner.submit_stop(Arc::clone(&audio_output)).unwrap();

        assert!(matches!(
            inner.wait_for_command(),
            Some(PlayerCommand::Stop { generation: 2 })
        ));
    }

    #[test]
    fn submit_play_fails_after_shutdown() {
        let inner = PlayerInner::default();
        let audio_output = AudioOutputBuffer::default();
        inner.shutdown(&audio_output);

        let error = inner
            .submit_play(
                RealtimePlaybackSchedule::new(vec![], 10),
                Arc::new(AudioOutputBuffer::default()),
            )
            .unwrap_err();

        assert!(error.to_string().contains("stopped"));
    }

    #[test]
    fn wait_for_command_returns_none_after_shutdown() {
        let inner = PlayerInner::default();
        let audio_output = AudioOutputBuffer::default();
        inner.shutdown(&audio_output);

        assert!(inner.wait_for_command().is_none());
    }
}
