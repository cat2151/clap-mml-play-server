//! オフラインレンダリングループ

use anyhow::Result;
use clack_host::prelude::*;
use clack_host::events::Match;
use clack_host::events::event_types::{NoteOnEvent, NoteOffEvent};
use clack_extensions::state::PluginState;
use hound::{WavSpec, WavWriter, SampleFormat};

use crate::CoreConfig;
use crate::midi::{TimedMidiEvent, MidiEvent};
use crate::host::{MidiRenderHost, MidiRenderHostShared};

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
fn load_patch(plugin_instance: &mut PluginInstance<MidiRenderHost>, patch_path: &str) -> Result<()> {
    let state_ext: PluginState = {
        let handle = plugin_instance.plugin_handle();
        handle.get_extension::<PluginState>()
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
    state_ext.load(&mut handle, &mut cursor)
        .map_err(|_| anyhow::anyhow!("パッチのロードに失敗: {}", patch_path))?;

    Ok(())
}

#[allow(dead_code)]
pub fn render(
    cfg: &CoreConfig,
    entry: &PluginEntry,
    events: Vec<TimedMidiEvent>,
    total_samples: u64,
) -> Result<()> {
    let plugin_factory = entry
        .get_plugin_factory()
        .ok_or_else(|| anyhow::anyhow!("PluginFactory が見つからない"))?;
    let plugin_descriptor = plugin_factory
        .plugin_descriptors()
        .next()
        .ok_or_else(|| anyhow::anyhow!("プラグインディスクリプタが見つからない"))?;
    let _plugin_name = plugin_descriptor.name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());

    let host_info = HostInfo::new("clap-midi-render", "clap-midi-render", "https://example.com", "0.1.0")?;
    let mut plugin_instance = PluginInstance::<MidiRenderHost>::new(
        |_| MidiRenderHostShared, |_| (), entry, plugin_descriptor.id().unwrap(), &host_info,
    )?;

    if let Some(ref patch) = cfg.patch_path {
        load_patch(&mut plugin_instance, patch)?;
    }

    let audio_config = PluginAudioConfiguration {
        sample_rate: cfg.sample_rate,
        min_frames_count: cfg.buffer_size as u32,
        max_frames_count: cfg.buffer_size as u32,
    };
    let audio_processor = plugin_instance.activate(|_, _| (), audio_config)?;

    let spec = WavSpec { channels: 2, sample_rate: cfg.sample_rate as u32, bits_per_sample: 32, sample_format: SampleFormat::Float };
    let mut wav = WavWriter::create(&cfg.output_wav, spec)
        .map_err(|e| anyhow::anyhow!("WAVファイルの作成に失敗: {}", e))?;

    let buf_size = cfg.buffer_size;
    let audio_processor = std::thread::scope(|s| {
        s.spawn(|| -> Result<_> {
            let mut ap = audio_processor.start_processing()
                .map_err(|e| anyhow::anyhow!("start_processing 失敗: {:?}", e))?;
            let mut current_sample: u64 = 0;
            let mut event_cursor: usize = 0;
            let mut in_left  = vec![0.0f32; buf_size];
            let mut in_right = vec![0.0f32; buf_size];
            let mut out_left  = vec![0.0f32; buf_size];
            let mut out_right = vec![0.0f32; buf_size];
            let mut input_ports  = AudioPorts::with_capacity(2, 1);
            let mut output_ports = AudioPorts::with_capacity(2, 1);
            let mut output_events_buf = EventBuffer::new();

            while current_sample < total_samples {
                let frames = buf_size.min((total_samples - current_sample) as usize) as u32;
                let buf_end = current_sample + frames as u64;
                let mut input_events_raw = EventBuffer::new();
                while event_cursor < events.len() && events[event_cursor].sample_pos < buf_end {
                    let ev = &events[event_cursor];
                    let offset = (ev.sample_pos.saturating_sub(current_sample)) as u32;
                    match ev.message {
                        MidiEvent::NoteOn { channel, key, velocity } => {
                            input_events_raw.push(&NoteOnEvent::new(offset, Pckn::new(0u16, channel as u16, key as u16, Match::All), velocity as f64 / 127.0));
                        }
                        MidiEvent::NoteOff { channel, key, velocity } => {
                            input_events_raw.push(&NoteOffEvent::new(offset, Pckn::new(0u16, channel as u16, key as u16, Match::All), velocity as f64 / 127.0));
                        }
                    }
                    event_cursor += 1;
                }
                let input_events = InputEvents::from_buffer(&input_events_raw);
                let mut output_events = OutputEvents::from_buffer(&mut output_events_buf);
                let in_l: &mut [f32] = &mut in_left[..frames as usize];
                let in_r: &mut [f32] = &mut in_right[..frames as usize];
                let out_l: &mut [f32] = &mut out_left[..frames as usize];
                let out_r: &mut [f32] = &mut out_right[..frames as usize];
                let input_audio = input_ports.with_input_buffers([AudioPortBuffer {
                    latency: 0, channels: AudioPortBufferType::f32_input_only([in_l, in_r].into_iter().map(InputChannel::constant)),
                }]);
                let mut output_audio = output_ports.with_output_buffers([AudioPortBuffer {
                    latency: 0, channels: AudioPortBufferType::f32_output_only([out_l, out_r].into_iter()),
                }]);
                ap.process(&input_audio, &mut output_audio, &input_events, &mut output_events, None, None)
                    .map_err(|e| anyhow::anyhow!("process() 失敗: {:?}", e))?;
                for i in 0..frames as usize {
                    wav.write_sample(out_left[i]).map_err(|e| anyhow::anyhow!("WAV 書き込み失敗: {}", e))?;
                    wav.write_sample(out_right[i]).map_err(|e| anyhow::anyhow!("WAV 書き込み失敗: {}", e))?;
                }
                current_sample = buf_end;
            }
            Ok(ap.stop_processing())
        }).join().unwrap()
    })?;

    plugin_instance.deactivate(audio_processor);
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
    let plugin_factory = entry
        .get_plugin_factory()
        .ok_or_else(|| anyhow::anyhow!("PluginFactory が見つからない"))?;
    let plugin_descriptor = plugin_factory
        .plugin_descriptors()
        .next()
        .ok_or_else(|| anyhow::anyhow!("プラグインディスクリプタが見つからない"))?;

    let host_info = HostInfo::new("clap-midi-render", "clap-midi-render", "https://example.com", "0.1.0")?;
    let mut plugin_instance = PluginInstance::<MidiRenderHost>::new(
        |_| MidiRenderHostShared, |_| (), entry, plugin_descriptor.id().unwrap(), &host_info,
    )?;

    if let Some(ref patch) = cfg.patch_path {
        load_patch(&mut plugin_instance, patch)?;
    }

    let audio_config = PluginAudioConfiguration {
        sample_rate: cfg.sample_rate,
        min_frames_count: cfg.buffer_size as u32,
        max_frames_count: cfg.buffer_size as u32,
    };
    let audio_processor = plugin_instance.activate(|_, _| (), audio_config)?;

    let buf_size = cfg.buffer_size;
    let (audio_processor, samples) = std::thread::scope(|s| {
        s.spawn(|| -> Result<_> {
            let mut ap = audio_processor.start_processing()
                .map_err(|e| anyhow::anyhow!("start_processing 失敗: {:?}", e))?;
            let mut current_sample: u64 = 0;
            let mut event_cursor: usize = 0;
            let mut out_samples: Vec<f32> = Vec::with_capacity(total_samples as usize * 2);
            let mut in_left  = vec![0.0f32; buf_size];
            let mut in_right = vec![0.0f32; buf_size];
            let mut out_left  = vec![0.0f32; buf_size];
            let mut out_right = vec![0.0f32; buf_size];
            let mut input_ports  = AudioPorts::with_capacity(2, 1);
            let mut output_ports = AudioPorts::with_capacity(2, 1);
            let mut output_events_buf = EventBuffer::new();

            while current_sample < total_samples {
                let frames = buf_size.min((total_samples - current_sample) as usize) as u32;
                let buf_end = current_sample + frames as u64;
                let mut input_events_raw = EventBuffer::new();
                while event_cursor < events.len() && events[event_cursor].sample_pos < buf_end {
                    let ev = &events[event_cursor];
                    let offset = (ev.sample_pos.saturating_sub(current_sample)) as u32;
                    match ev.message {
                        MidiEvent::NoteOn { channel, key, velocity } => {
                            input_events_raw.push(&NoteOnEvent::new(offset, Pckn::new(0u16, channel as u16, key as u16, Match::All), velocity as f64 / 127.0));
                        }
                        MidiEvent::NoteOff { channel, key, velocity } => {
                            input_events_raw.push(&NoteOffEvent::new(offset, Pckn::new(0u16, channel as u16, key as u16, Match::All), velocity as f64 / 127.0));
                        }
                    }
                    event_cursor += 1;
                }
                let input_events = InputEvents::from_buffer(&input_events_raw);
                let mut output_events = OutputEvents::from_buffer(&mut output_events_buf);
                let in_l: &mut [f32] = &mut in_left[..frames as usize];
                let in_r: &mut [f32] = &mut in_right[..frames as usize];
                let out_l: &mut [f32] = &mut out_left[..frames as usize];
                let out_r: &mut [f32] = &mut out_right[..frames as usize];
                let input_audio = input_ports.with_input_buffers([AudioPortBuffer {
                    latency: 0, channels: AudioPortBufferType::f32_input_only([in_l, in_r].into_iter().map(InputChannel::constant)),
                }]);
                let mut output_audio = output_ports.with_output_buffers([AudioPortBuffer {
                    latency: 0, channels: AudioPortBufferType::f32_output_only([out_l, out_r].into_iter()),
                }]);
                ap.process(&input_audio, &mut output_audio, &input_events, &mut output_events, None, None)
                    .map_err(|e| anyhow::anyhow!("process() 失敗: {:?}", e))?;
                for i in 0..frames as usize {
                    out_samples.push(out_left[i]);
                    out_samples.push(out_right[i]);
                }
                current_sample = buf_end;
            }
            Ok((ap.stop_processing(), out_samples))
        }).join().unwrap()
    })?;

    plugin_instance.deactivate(audio_processor);
    Ok(samples)
}
