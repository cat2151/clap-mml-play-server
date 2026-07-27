//! スケジュール済み MIDI イベント列を最後までレンダリングする入口。
//!
//! `RealtimeRenderer` を chunk 単位で回すだけの薄いループで、出力先が WAV ファイルか
//! メモリ上の `Vec<f32>` かだけが違う。

use anyhow::Result;
use clack_host::prelude::*;
use hound::{SampleFormat, WavSpec, WavWriter};

use super::{RealtimePlaybackSchedule, RealtimeRenderer};
use crate::midi::TimedMidiEvent;
use crate::CoreConfig;

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
