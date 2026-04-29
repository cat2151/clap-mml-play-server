//! オフラインレンダリングループ

use anyhow::Result;
use clack_extensions::state::PluginState;
use clack_host::events::event_types::{NoteOffEvent, NoteOnEvent};
use clack_host::events::Match;
use clack_host::prelude::*;
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::host::{MidiRenderHost, MidiRenderHostShared};
use crate::midi::{MidiEvent, TimedMidiEvent};
use crate::CoreConfig;

/// .fxp ファイルを clap state として plugin にロードする
///
/// Surge XTの .fxp は VST2 の opaque chunk 形式:
///   Bytes  0-3 : 'CcnK'
///   Bytes  4-7 : byteSize (big-endian)
///   Bytes  8-11: 'FPCh' (opaque chunk preset)
///   Bytes 12-27: version / fxID / fxVersion / numPrograms
///   Bytes 28-31: chunkSize (big-endian)
///   Bytes 32+  : chunk data (== Surge 独自形式: 'sub3' + xml + wavetables)
///
/// CLAP state として渡すべきは chunk data (offset 32 以降) のみ。
fn load_patch(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    patch_path: &str,
) -> Result<()> {
    let state_ext: PluginState = {
        let handle = plugin_instance.plugin_handle();
        handle
            .get_extension::<PluginState>()
            .ok_or_else(|| anyhow::anyhow!("プラグインが state extension をサポートしていない"))?
    }; // handle をここでドロップ

    let raw = std::fs::read(patch_path)
        .map_err(|e| anyhow::anyhow!("パッチファイルを読めない '{}': {}", patch_path, e))?;

    // FXP ヘッダを検出して chunk data だけを切り出す
    //
    // Surge XT の .fxp は標準FXPと異なる独自レイアウト:
    //   offset  0- 3: 'CcnK'
    //   offset  4- 7: byteSize (big-endian, Surgeは0埋め)
    //   offset  8-11: 'FPCh'
    //   offset 12-27: version / fxID('cjs3') / fxVersion / numPrograms
    //   offset 28-55: プリセット名等 (28バイト, 独自フィールド)
    //   offset 56-59: chunkSize (big-endian)
    //   offset 60+  : chunk data ('sub3' + xml + wavetables)
    let chunk_data: &[u8] = if raw.len() >= 60 && &raw[0..4] == b"CcnK" && &raw[8..12] == b"FPCh" {
        // 'sub3' が offset 60 にあることを確認
        if &raw[60..64] == b"sub3" {
            let chunk_size = u32::from_be_bytes([raw[56], raw[57], raw[58], raw[59]]) as usize;
            let end = (60 + chunk_size).min(raw.len());
            &raw[60..end]
        } else {
            // 念のため 'sub3' をスキャンして見つける
            let pos = raw.windows(4).position(|w| w == b"sub3").unwrap_or(0);
            &raw[pos..]
        }
    } else {
        // FXP ヘッダなし: そのまま渡す（'sub3' 形式か XML か）
        &raw[..]
    };

    let mut cursor = std::io::Cursor::new(chunk_data);
    let mut handle = plugin_instance.plugin_handle();
    state_ext
        .load(&mut handle, &mut cursor)
        .map_err(|_| anyhow::anyhow!("パッチのロードに失敗: {}", patch_path))?;

    Ok(())
}

pub fn create_plugin_instance(
    cfg: &CoreConfig,
    entry: &PluginEntry,
) -> Result<PluginInstance<MidiRenderHost>> {
    let plugin_factory = entry
        .get_plugin_factory()
        .ok_or_else(|| anyhow::anyhow!("PluginFactory が見つからない"))?;
    let plugin_descriptor = plugin_factory
        .plugin_descriptors()
        .next()
        .ok_or_else(|| anyhow::anyhow!("プラグインディスクリプタが見つからない"))?;

    let host_info = HostInfo::new(
        "clap-midi-render",
        "clap-midi-render",
        "https://example.com",
        "0.1.0",
    )?;
    let mut plugin_instance = PluginInstance::<MidiRenderHost>::new(
        |_| MidiRenderHostShared,
        |_| (),
        entry,
        plugin_descriptor.id().unwrap(),
        &host_info,
    )?;

    if let Some(ref patch) = cfg.patch_path {
        load_patch(&mut plugin_instance, patch)?;
    }

    Ok(plugin_instance)
}

#[derive(Debug, Clone)]
pub struct RealtimePlaybackSchedule {
    events: Vec<TimedMidiEvent>,
    total_samples: u64,
    current_sample: u64,
    event_cursor: usize,
}

impl RealtimePlaybackSchedule {
    pub fn new(events: Vec<TimedMidiEvent>, total_samples: u64) -> Self {
        Self {
            events,
            total_samples,
            current_sample: 0,
            event_cursor: 0,
        }
    }

    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    pub fn current_sample(&self) -> u64 {
        self.current_sample
    }

    pub fn events(&self) -> &[TimedMidiEvent] {
        &self.events
    }

    pub fn is_finished(&self) -> bool {
        self.current_sample >= self.total_samples
    }
}

pub struct RealtimeRenderer {
    plugin_instance: Option<PluginInstance<MidiRenderHost>>,
    processor: Option<StartedPluginAudioProcessor<MidiRenderHost>>,
    buf_size: usize,
    in_left: Vec<f32>,
    in_right: Vec<f32>,
    out_left: Vec<f32>,
    out_right: Vec<f32>,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    output_events_buf: EventBuffer,
}

impl RealtimeRenderer {
    pub fn new(cfg: &CoreConfig, entry: &PluginEntry) -> Result<Self> {
        let mut plugin_instance = create_plugin_instance(cfg, entry)?;
        let audio_config = PluginAudioConfiguration {
            sample_rate: cfg.sample_rate,
            min_frames_count: cfg.buffer_size as u32,
            max_frames_count: cfg.buffer_size as u32,
        };
        let audio_processor = plugin_instance.activate(|_, _| (), audio_config)?;
        let processor = audio_processor
            .start_processing()
            .map_err(|e| anyhow::anyhow!("start_processing 失敗: {:?}", e))?;

        Ok(Self {
            plugin_instance: Some(plugin_instance),
            processor: Some(processor),
            buf_size: cfg.buffer_size,
            in_left: vec![0.0; cfg.buffer_size],
            in_right: vec![0.0; cfg.buffer_size],
            out_left: vec![0.0; cfg.buffer_size],
            out_right: vec![0.0; cfg.buffer_size],
            input_ports: AudioPorts::with_capacity(2, 1),
            output_ports: AudioPorts::with_capacity(2, 1),
            output_events_buf: EventBuffer::new(),
        })
    }

    pub fn reset(&mut self) {
        if let Some(processor) = self.processor.as_mut() {
            processor.reset();
        }
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

        let input_events = InputEvents::from_buffer(&input_events_raw);
        let mut output_events = OutputEvents::from_buffer(&mut self.output_events_buf);
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
        self.processor
            .as_mut()
            .expect("processor is always present while renderer is alive")
            .process(
                &input_audio,
                &mut output_audio,
                &input_events,
                &mut output_events,
                None,
                None,
            )
            .map_err(|e| anyhow::anyhow!("process() 失敗: {:?}", e))?;

        let mut samples = Vec::with_capacity(frame_len * 2);
        for i in 0..frame_len {
            samples.push(self.out_left[i]);
            samples.push(self.out_right[i]);
        }
        playback.current_sample = buf_end;
        Ok(Some(samples))
    }
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

#[allow(dead_code)]
pub fn render(
    cfg: &CoreConfig,
    entry: &PluginEntry,
    events: Vec<TimedMidiEvent>,
    total_samples: u64,
) -> Result<()> {
    let spec = WavSpec {
        channels: 2,
        sample_rate: cfg.sample_rate as u32,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut wav = WavWriter::create(&cfg.output_wav, spec)
        .map_err(|e| anyhow::anyhow!("WAVファイルの作成に失敗: {}", e))?;

    let mut renderer = RealtimeRenderer::new(cfg, entry)?;
    let mut playback = RealtimePlaybackSchedule::new(events, total_samples);
    while let Some(chunk) = renderer.render_next_chunk(&mut playback)? {
        for sample in chunk {
            wav.write_sample(sample)
                .map_err(|e| anyhow::anyhow!("WAV 書き込み失敗: {}", e))?;
        }
    }
    wav.finalize()?;
    Ok(())
}

/// メモリ上にレンダリングして Vec<f32>（インターリーブステレオ）を返す
pub fn render_to_memory(
    cfg: &CoreConfig,
    entry: &PluginEntry,
    events: Vec<TimedMidiEvent>,
    total_samples: u64,
) -> Result<Vec<f32>> {
    let mut renderer = RealtimeRenderer::new(cfg, entry)?;
    let mut playback = RealtimePlaybackSchedule::new(events, total_samples);
    let mut samples = Vec::with_capacity(total_samples as usize * 2);
    while let Some(chunk) = renderer.render_next_chunk(&mut playback)? {
        samples.extend(chunk);
    }
    Ok(samples)
}
