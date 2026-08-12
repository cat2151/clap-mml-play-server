//! オフラインレンダリングループ

use std::time::{Duration, Instant};

use anyhow::Result;
use clack_host::events::event_types::{MidiEvent as ClapMidiEvent, NoteOffEvent, NoteOnEvent};
use clack_host::events::spaces::CoreEventSpace;
use clack_host::events::{EventFlags, Match};
use clack_host::prelude::*;
use cmrt_clack_timeline::{process_block_timing, ProcessBlockTiming};
use cmrt_timeline::{BlockSpan, FreeRunningTimeline, SamplePosition, SampleRate};

use crate::host::MidiRenderHost;
use crate::midi::MidiEvent;
use crate::CoreConfig;

mod instance;
mod offline;
mod parallel;
mod patch_state;
mod playback;
mod voicing_probe;
use instance::create_plugin_instance_without_patch;
use patch_state::{load_patch, load_plugin_state, save_plugin_state};
use voicing_probe::first_plugin_id;

pub use instance::create_plugin_instance;
pub use offline::{render, render_to_memory};
pub use parallel::{create_renderers_parallel, RendererCreated};
pub use playback::RealtimePlaybackSchedule;

/// live MIDI 1.0 short message と、その chunk 先頭からのフレームオフセット。
///
/// オフセットは呼び出し側が chunk 境界へ割り付け済みであること。`render_live_chunk_with_offsets`
/// は `buf_size - 1` を超えるオフセットをクランプするだけで、次 chunk へ持ち越さない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveMidiEvent {
    pub offset_frames: u32,
    pub message: [u8; 3],
}

pub struct RealtimeRenderer {
    plugin_instance: Option<PluginInstance<MidiRenderHost>>,
    processor: Option<StartedPluginAudioProcessor<MidiRenderHost>>,
    /// パッチロード前の初期 state（Init Saw）。set_patch(None) で復元する。
    init_state: Option<Vec<u8>>,
    /// 現在ロードされているパッチのパス。None は初期 state。
    current_patch: Option<String>,
    plugin_id: String,
    buf_size: usize,
    in_left: Vec<f32>,
    in_right: Vec<f32>,
    out_left: Vec<f32>,
    out_right: Vec<f32>,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    output_events_buf: EventBuffer,
    /// CLAP `steady_time` is activation-local and never moves backwards, including across a
    /// musical transport reset or a patch probe.
    process_cursor_samples: u64,
    sample_rate: SampleRate,
}

/// `RealtimeRenderer::new_with_timing` が返す、インスタンス生成の内訳。
///
/// サーバー起動時間の大部分は CLAP インスタンスの生成が占めるため、その中の
/// どの操作が支配的かを呼び出し側から計測できるようにしている。ログ形式を
/// ライブラリ側に持ち込まないよう、値を返すだけにしてある。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RendererInitTiming {
    /// `first_plugin_id()`（factory 取得 + descriptor 走査）。
    pub plugin_id: Duration,
    /// CLAP の `create_plugin()` + `init()`。
    pub instantiate: Duration,
    /// 初期 state のスナップショット。
    pub save_state: Duration,
    /// `cfg.patch_path` が `Some` のときのみ非ゼロ。
    pub load_patch: Duration,
    /// `activate()`。
    pub activate: Duration,
    /// `start_processing()`。
    pub start_processing: Duration,
    /// 上記すべてとバッファ確保を含む合計。
    pub total: Duration,
}

impl RealtimeRenderer {
    pub fn new(cfg: &CoreConfig, entry: &PluginEntry) -> Result<Self> {
        Self::new_with_timing(cfg, entry).map(|(renderer, _)| renderer)
    }

    /// `new()` と同じ処理を行い、内訳の所要時間も返す。
    pub fn new_with_timing(
        cfg: &CoreConfig,
        entry: &PluginEntry,
    ) -> Result<(Self, RendererInitTiming)> {
        let started = Instant::now();
        let mut timing = RendererInitTiming::default();

        let step = Instant::now();
        let plugin_id = first_plugin_id(entry)?;
        timing.plugin_id = step.elapsed();

        let step = Instant::now();
        let mut plugin_instance = create_plugin_instance_without_patch(entry)?;
        timing.instantiate = step.elapsed();

        // パッチ未指定の再生で初期音色（Init Saw）へ戻せるよう、ロード前の state を取っておく。
        // state extension が無いプラグインでは None（set_patch(None) がエラーになるだけ）。
        let step = Instant::now();
        let init_state = save_plugin_state(&mut plugin_instance).ok();
        timing.save_state = step.elapsed();

        if let Some(ref patch) = cfg.patch_path {
            let step = Instant::now();
            load_patch(&mut plugin_instance, patch)?;
            timing.load_patch = step.elapsed();
        }
        let audio_config = PluginAudioConfiguration {
            sample_rate: cfg.sample_rate,
            min_frames_count: cfg.buffer_size as u32,
            max_frames_count: cfg.buffer_size as u32,
        };
        let step = Instant::now();
        let audio_processor = plugin_instance.activate(|_, _| (), audio_config)?;
        timing.activate = step.elapsed();

        let step = Instant::now();
        let processor = audio_processor
            .start_processing()
            .map_err(|e| anyhow::anyhow!("start_processing 失敗: {:?}", e))?;
        timing.start_processing = step.elapsed();

        let renderer = Self {
            plugin_instance: Some(plugin_instance),
            processor: Some(processor),
            init_state,
            current_patch: cfg.patch_path.clone(),
            plugin_id,
            buf_size: cfg.buffer_size,
            in_left: vec![0.0; cfg.buffer_size],
            in_right: vec![0.0; cfg.buffer_size],
            out_left: vec![0.0; cfg.buffer_size],
            out_right: vec![0.0; cfg.buffer_size],
            input_ports: AudioPorts::with_capacity(2, 1),
            output_ports: AudioPorts::with_capacity(2, 1),
            output_events_buf: EventBuffer::new(),
            process_cursor_samples: 0,
            sample_rate: SampleRate::new(cfg.sample_rate)
                .map_err(|error| anyhow::anyhow!(error))?,
        };
        timing.total = started.elapsed();
        Ok((renderer, timing))
    }

    pub fn reset(&mut self) {
        if let Some(processor) = self.processor.as_mut() {
            processor.reset();
        }
    }

    /// 再生前にパッチを切り替える。同一パッチなら何もしない。
    /// `None` は生成直後にスナップショットした初期 state（Init Saw）へ戻す。
    /// state のロードは main-thread 操作のため、process() と直列なスレッドから呼ぶこと。
    pub fn set_patch(&mut self, patch: Option<&str>) -> Result<()> {
        if self.current_patch.as_deref() == patch {
            return Ok(());
        }
        let plugin_instance = self
            .plugin_instance
            .as_mut()
            .expect("plugin instance is always present while renderer is alive");
        match patch {
            Some(path) => load_patch(plugin_instance, path)?,
            None => {
                let init_state = self.init_state.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("初期 state が未取得のため初期音色へ戻せない")
                })?;
                load_plugin_state(plugin_instance, init_state)
                    .map_err(|e| anyhow::anyhow!("初期 state の復元に失敗: {}", e))?;
            }
        }
        self.current_patch = patch.map(str::to_string);
        Ok(())
    }

    pub fn render_next_chunk(
        &mut self,
        playback: &mut RealtimePlaybackSchedule,
    ) -> Result<Option<Vec<f32>>> {
        if playback.is_finished() {
            return Ok(None);
        }

        let frames = self
            .buf_size
            .min((playback.total_samples - playback.current_sample) as usize)
            as u32;
        let buf_end = playback.current_sample + frames as u64;
        let mut input_events_raw = EventBuffer::new();
        while playback.event_cursor < playback.events.len()
            && playback.events[playback.event_cursor].sample_pos < buf_end
        {
            let ev = &playback.events[playback.event_cursor];
            let offset = (ev.sample_pos.saturating_sub(playback.current_sample)) as u32;
            match ev.message {
                MidiEvent::NoteOn {
                    channel,
                    key,
                    velocity,
                } => {
                    input_events_raw.push(&NoteOnEvent::new(
                        offset,
                        Pckn::new(0u16, channel as u16, key as u16, Match::All),
                        velocity as f64 / 127.0,
                    ));
                }
                MidiEvent::NoteOff {
                    channel,
                    key,
                    velocity,
                } => {
                    input_events_raw.push(&NoteOffEvent::new(
                        offset,
                        Pckn::new(0u16, channel as u16, key as u16, Match::All),
                        velocity as f64 / 127.0,
                    ));
                }
            }
            playback.event_cursor += 1;
        }

        let timing = self.playback_block_timing(playback, frames)?;
        let samples = self
            .process_chunk_with_timing(frames, &input_events_raw, timing)
            .map(|processed| processed.samples)?;
        playback.current_sample = buf_end;
        Ok(Some(samples))
    }

    /// オフライン／スケジュール再生のブロックタイミング。
    ///
    /// `steady_time` は renderer ローカルで単調（プラグインが自由走行の時計として使う）、
    /// transport は曲の tempo map から作る。曲の位置は renderer のカーソルではなく
    /// スケジュールの再生位置で決めること（renderer は使い回されうる）。
    fn playback_block_timing(
        &self,
        playback: &RealtimePlaybackSchedule,
        frames: u32,
    ) -> Result<ProcessBlockTiming> {
        let Some(transport) = playback.transport.as_ref() else {
            let block = BlockSpan::new(SamplePosition(self.process_cursor_samples), frames)
                .map_err(|error| anyhow::anyhow!(error))?;
            return Ok(process_block_timing(
                block,
                self.sample_rate,
                &FreeRunningTimeline,
            ));
        };
        let musical_block = BlockSpan::new(SamplePosition(playback.musical_sample()), frames)
            .map_err(|error| anyhow::anyhow!(error))?;
        let mut timing = process_block_timing(musical_block, self.sample_rate, transport);
        timing.steady_time = self.process_cursor_samples;
        Ok(timing)
    }

    /// 1 chunk のフレーム数。live のスケジューラが chunk 境界を計算するのに使う。
    pub fn buf_size(&self) -> usize {
        self.buf_size
    }

    /// timestampを持たないlive MIDI 1.0 short message群を、順序を保って
    /// 次のchunk先頭で処理する。複数のNote Onは同時発音としてpluginへ渡る。
    pub fn render_live_chunk(&mut self, midi_messages: &[[u8; 3]]) -> Result<Vec<f32>> {
        let events = midi_messages
            .iter()
            .map(|message| LiveMidiEvent {
                offset_frames: 0,
                message: *message,
            })
            .collect::<Vec<_>>();
        self.render_live_chunk_with_offsets(&events)
    }

    /// chunk 内オフセット付きの live 描画。オフセットはサンプル精度でpluginへ渡る。
    ///
    /// イベントは `offset_frames` 昇順で渡すこと（CLAP のイベントリストは時刻順が前提）。
    pub fn render_live_chunk_with_offsets(&mut self, events: &[LiveMidiEvent]) -> Result<Vec<f32>> {
        let block = BlockSpan::new(
            SamplePosition(self.process_cursor_samples),
            self.buf_size as u32,
        )
        .map_err(|error| anyhow::anyhow!(error))?;
        let timing = process_block_timing(block, self.sample_rate, &FreeRunningTimeline);
        self.render_live_chunk_with_timing(events, timing)
    }

    /// Render a live block with an explicit musical transport snapshot. `steady_time` remains
    /// renderer-local and monotonic; callers may reset the musical timeline independently.
    pub fn render_live_chunk_with_timing(
        &mut self,
        events: &[LiveMidiEvent],
        mut timing: ProcessBlockTiming,
    ) -> Result<Vec<f32>> {
        let last_frame = self.buf_size.saturating_sub(1) as u32;
        let mut input_events_raw = EventBuffer::new();
        for event in events {
            let offset = event.offset_frames.min(last_frame);
            input_events_raw.push(
                &ClapMidiEvent::new(offset, 0, event.message).with_flags(EventFlags::IS_LIVE),
            );
        }
        timing.steady_time = self.process_cursor_samples;
        self.process_chunk_with_timing(self.buf_size as u32, &input_events_raw, timing)
            .map(|processed| processed.samples)
    }

    fn process_chunk_with_events(
        &mut self,
        frames: u32,
        input_events_raw: &EventBuffer,
    ) -> Result<ProcessedChunk> {
        let block = BlockSpan::new(SamplePosition(self.process_cursor_samples), frames)
            .map_err(|error| anyhow::anyhow!(error))?;
        let timing = process_block_timing(block, self.sample_rate, &FreeRunningTimeline);
        self.process_chunk_with_timing(frames, input_events_raw, timing)
    }

    fn process_chunk_with_timing(
        &mut self,
        frames: u32,
        input_events_raw: &EventBuffer,
        timing: ProcessBlockTiming,
    ) -> Result<ProcessedChunk> {
        let input_events = InputEvents::from_buffer(input_events_raw);
        self.output_events_buf.clear();
        let frame_len = frames as usize;
        self.out_left[..frame_len].fill(0.0);
        self.out_right[..frame_len].fill(0.0);
        let in_l: &mut [f32] = &mut self.in_left[..frame_len];
        let in_r: &mut [f32] = &mut self.in_right[..frame_len];
        let out_l: &mut [f32] = &mut self.out_left[..frame_len];
        let out_r: &mut [f32] = &mut self.out_right[..frame_len];
        let input_audio = self.input_ports.with_input_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                [in_l, in_r].into_iter().map(InputChannel::constant),
            ),
        }]);
        let mut output_audio = self.output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only([out_l, out_r].into_iter()),
        }]);
        {
            let mut output_events = OutputEvents::from_buffer(&mut self.output_events_buf);
            self.processor
                .as_mut()
                .expect("processor is always present while renderer is alive")
                .process(
                    &input_audio,
                    &mut output_audio,
                    &input_events,
                    &mut output_events,
                    Some(timing.steady_time),
                    timing.transport.as_ref(),
                )
                .map_err(|e| anyhow::anyhow!("process() 失敗: {:?}", e))?;
        }
        self.process_cursor_samples = self
            .process_cursor_samples
            .saturating_add(u64::from(frames));

        let ended_note_ids = self
            .output_events_buf
            .iter()
            .filter_map(|event| match event.as_core_event() {
                Some(CoreEventSpace::NoteEnd(note_end)) => note_end.pckn().note_id.into_specific(),
                _ => None,
            })
            .collect();

        let mut samples = Vec::with_capacity(frame_len * 2);
        for i in 0..frame_len {
            samples.push(self.out_left[i]);
            samples.push(self.out_right[i]);
        }
        Ok(ProcessedChunk {
            samples,
            ended_note_ids,
        })
    }
}

struct ProcessedChunk {
    samples: Vec<f32>,
    ended_note_ids: Vec<u32>,
}

impl Drop for RealtimeRenderer {
    fn drop(&mut self) {
        let Some(processor) = self.processor.take() else {
            return;
        };
        let Some(mut plugin_instance) = self.plugin_instance.take() else {
            return;
        };
        let stopped = processor.stop_processing();
        plugin_instance.deactivate(stopped);
    }
}
