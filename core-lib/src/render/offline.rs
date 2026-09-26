//! スケジュール済み MIDI イベント列を最後までレンダリングする入口。
//!
//! `RealtimeRenderer` を chunk 単位で回すだけの薄いループで、出力先が WAV ファイルか
//! メモリ上の `Vec<f32>` かだけが違う。
//!
//! instance 生成（instantiate + 音色ロード）はプロセス全体で 1 本ずつ。sforzando は 2 スレッドで
//! 重なると access violation でプロセスごと落ち、Floe は instantiate に失敗する。守る区間が
//! `create_plugin()` + `init()` の外（音色ロード）まで要るので [`super::serial_instantiation`] では
//! 届かず、plugin 別にすると未知の plugin で同じ調査を繰り返すので、全 plugin 一律にしてある
//! （`docs/adr/0021-offline-render-serializes-instance-creation.md`）。`render_next_chunk` は
//! lock の外で並列のまま。

use std::sync::Mutex;

use anyhow::Result;
use clack_host::prelude::*;
use hound::{SampleFormat, WavSpec, WavWriter};

use super::{RealtimePlaybackSchedule, RealtimeRenderer};
use crate::CoreConfig;

static INSTANCE_CREATION: Mutex<()> = Mutex::new(());

fn create_renderer_one_at_a_time(
    cfg: &CoreConfig,
    entry: &PluginEntry,
) -> Result<RealtimeRenderer> {
    // 他スレッドが生成中に panic して毒されていても、守るのは「同時に走らせない」ことだけ。
    let _guard = INSTANCE_CREATION.lock().unwrap_or_else(|e| e.into_inner());
    RealtimeRenderer::new_for_offline(cfg, entry)
}

#[allow(dead_code)]
pub fn render(
    cfg: &CoreConfig,
    entry: &PluginEntry,
    mut playback: RealtimePlaybackSchedule,
) -> Result<()> {
    let spec = WavSpec {
        channels: 2,
        sample_rate: cfg.sample_rate as u32,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut wav = WavWriter::create(&cfg.output_wav, spec)
        .map_err(|e| anyhow::anyhow!("WAVファイルの作成に失敗: {}", e))?;

    let mut renderer = create_renderer_one_at_a_time(cfg, entry)?;
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
    mut playback: RealtimePlaybackSchedule,
) -> Result<Vec<f32>> {
    let mut renderer = create_renderer_one_at_a_time(cfg, entry)?;
    let mut samples = Vec::with_capacity(playback.total_samples() as usize * 2);
    while let Some(chunk) = renderer.render_next_chunk(&mut playback)? {
        samples.extend(chunk);
    }
    Ok(samples)
}
