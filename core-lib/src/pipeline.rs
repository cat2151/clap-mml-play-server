//! MML → SMF → WAV → 再生 パイプライン

use anyhow::Result;
use clack_host::prelude::PluginEntry;

use crate::render::RealtimePlaybackSchedule;
use crate::CoreConfig;

use mmlabc_to_smf::{mml_preprocessor, raw_mml_to_smf_bytes_with_options, SmfConversionOptions};

mod audio;
mod effects;
mod history;
mod output_dirs;
mod patch_resolution;
mod rendering;
mod temp_dir;
#[cfg(test)]
mod test_support;

pub use audio::{encode_wav_i16, play_samples, write_wav};
pub use effects::{EffectEntryLoader, RenderEffects};
use history::append_history;
pub use output_dirs::{ensure_cmrt_dir, ensure_daw_dir, ensure_phrase_dir};
pub use patch_resolution::embedded_patch_ref;
use patch_resolution::{patch_display_for_render, resolve_effective_patch};
#[cfg(test)]
use rendering::{apply_render_preroll, trim_render_preroll};
use rendering::{
    prepare_playback_schedule, prepare_render_inputs, render_prepared_inputs, PreparedRenderInputs,
};
pub use rendering::{RenderOptions, RenderPreroll};
use temp_dir::RenderTempDir;
#[cfg(test)]
pub(crate) use test_support::{env_lock, EnvVarGuard};

/// MML → レンダリングのみ。再生はしない。
/// 戻り値: (サンプル列, 使用パッチ相対パス)
pub fn mml_render(mml: &str, cfg: &CoreConfig, entry: &PluginEntry) -> Result<(Vec<f32>, String)> {
    mml_render_with_options(mml, cfg, entry, RenderOptions::default())
}

/// MML → レンダリングのみ。`RenderOptions` で preroll などの追加処理を指定できる。
/// effect chain 付きの MML はエラー（[`mml_render_with_effects`] を使う）。
pub fn mml_render_with_options(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
) -> Result<(Vec<f32>, String)> {
    mml_render_with_effects(mml, cfg, entry, options, RenderEffects::unsupported())
}

/// MML → レンダリングのみ。先頭 JSON の effect chain を instrument の後段に通す。
pub fn mml_render_with_effects(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
    effects: RenderEffects,
) -> Result<(Vec<f32>, String)> {
    let prepared = prepare_phrase_render(mml, cfg, options, effects)?;
    let patch_display = prepared.patch_display;
    let output_wav = prepared.output_wav;
    let samples = render_prepared_inputs(prepared.inputs, entry, &effects)?;
    write_wav(&samples, cfg.sample_rate as u32, &output_wav)?;
    Ok((samples, patch_display))
}

/// MML → レンダリングのみ。履歴や永続 output.mid/output.wav は書かない。
pub fn mml_render_stateless(mml: &str, cfg: &CoreConfig, entry: &PluginEntry) -> Result<Vec<f32>> {
    mml_render_stateless_with_options(mml, cfg, entry, RenderOptions::default())
}

/// MML → レンダリングのみ。中間ファイルは生成しない。
/// effect chain 付きの MML はエラー（[`mml_render_stateless_with_effects`] を使う）。
pub fn mml_render_stateless_with_options(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
) -> Result<Vec<f32>> {
    mml_render_stateless_with_effects(mml, cfg, entry, options, RenderEffects::unsupported())
}

/// MML → レンダリングのみ。中間ファイルは生成しない。先頭 JSON の effect chain を通す。
pub fn mml_render_stateless_with_effects(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
    effects: RenderEffects,
) -> Result<Vec<f32>> {
    let temp_dir = RenderTempDir::create()?;
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let effective_patch =
        resolve_effective_patch(preprocessed.embedded_json.as_deref(), cfg, false)?;
    let chain = effects.chain_spec(preprocessed.embedded_json.as_deref())?;
    let smf_bytes = mml_str_to_smf_bytes(&preprocessed.remaining_mml)?;
    let patched_cfg = CoreConfig {
        output_midi: utf8_path_string(&temp_dir.path().join("output.mid"), "一時MIDIパス")?,
        output_wav: utf8_path_string(&temp_dir.path().join("output.wav"), "一時WAVパス")?,
        patch_path: effective_patch,
        random_patch: false,
        ..cfg.clone()
    };
    let inputs = prepare_render_inputs(&smf_bytes, patched_cfg, options, chain)?;
    render_prepared_inputs(inputs, entry, &effects)
}

/// SMF bytes → レンダリングのみ。中間ファイルは生成しない。
pub fn smf_render_stateless_with_options(
    smf_bytes: &[u8],
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
) -> Result<Vec<f32>> {
    let temp_dir = RenderTempDir::create()?;
    let patched_cfg = CoreConfig {
        output_midi: utf8_path_string(&temp_dir.path().join("output.mid"), "一時MIDIパス")?,
        output_wav: utf8_path_string(&temp_dir.path().join("output.wav"), "一時WAVパス")?,
        random_patch: false,
        ..cfg.clone()
    };
    // SMF には先頭 JSON が無いので effect chain は常に空。
    let inputs = prepare_render_inputs(smf_bytes, patched_cfg, options, Default::default())?;
    render_prepared_inputs(inputs, entry, &RenderEffects::unsupported())
}

/// SMF bytes → realtime playback 用イベント列。中間ファイルは生成しない。
pub fn smf_playback_schedule_with_options(
    smf_bytes: &[u8],
    sample_rate: f64,
    options: RenderOptions,
) -> Result<RealtimePlaybackSchedule> {
    prepare_playback_schedule(smf_bytes, sample_rate, options)
}

/// キャッシュ構築専用の MML → レンダリング。
/// - `patch_history.txt` への追記は行わない
/// - MIDI/WAV の出力先は DAW 専用ディレクトリ（`config_local_dir()/clap-mml-render-tui/daw/daw_cache.mid/wav`）を使用
///   することで通常の出力ファイルを上書きしない
/// - 呼び出し元はシリアルな単一ワーカースレッドから呼び出すこと（ファイル書き込みの
///   競合を防ぐため）
pub fn mml_render_for_cache(mml: &str, cfg: &CoreConfig, entry: &PluginEntry) -> Result<Vec<f32>> {
    mml_render_for_cache_with_options(mml, cfg, entry, RenderOptions::default())
}

/// キャッシュ構築専用の MML → レンダリング。`RenderOptions` で preroll などを指定できる。
/// effect chain 付きの MML はエラー（[`mml_render_for_cache_with_effects`] を使う）。
pub fn mml_render_for_cache_with_options(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
) -> Result<Vec<f32>> {
    mml_render_for_cache_with_effects(mml, cfg, entry, options, RenderEffects::unsupported())
}

/// キャッシュ構築専用の MML → レンダリング。先頭 JSON の effect chain を instrument の後段に通す。
pub fn mml_render_for_cache_with_effects(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
    effects: RenderEffects,
) -> Result<Vec<f32>> {
    let prepared = prepare_cache_render(mml, cfg, options, effects)?;
    let output_wav = prepared.output_wav;
    let samples = render_prepared_inputs(prepared.inputs, entry, &effects)?;
    write_wav(&samples, cfg.sample_rate as u32, &output_wav)?;

    Ok(samples)
}

/// MML文字列 → SMF・WAVファイル出力 + 即時再生
/// 優先順位:
///   1. MML先頭のJSON `{"Surge XT patch": "Pads/Pad 1.fxp"}` で指定されたパッチ
///   2. random_patch = true なら patches_dir からランダム選択
///   3. config.toml の patch_path
///   4. Init Saw（デフォルト）
///
/// 戻り値: 使用したパッチの相対パス（またはnone文字列）
pub fn mml_to_play(mml: &str, cfg: &CoreConfig, entry: &PluginEntry) -> Result<String> {
    mml_to_play_with_options(mml, cfg, entry, RenderOptions::default())
}

/// MML文字列 → SMF・WAVファイル出力 + 即時再生。`RenderOptions` で preroll などを指定できる。
pub fn mml_to_play_with_options(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
) -> Result<String> {
    let (samples, patch_display) = mml_render_with_options(mml, cfg, entry, options)?;
    play_samples(samples, cfg.sample_rate as u32)?;
    Ok(patch_display)
}

struct PreparedPhraseRender {
    inputs: PreparedRenderInputs,
    output_wav: std::path::PathBuf,
    patch_display: String,
}

struct PreparedCacheRender {
    inputs: PreparedRenderInputs,
    output_wav: std::path::PathBuf,
}

fn prepare_phrase_render(
    mml: &str,
    cfg: &CoreConfig,
    options: RenderOptions,
    effects: RenderEffects,
) -> Result<PreparedPhraseRender> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let effective_patch =
        resolve_effective_patch(preprocessed.embedded_json.as_deref(), cfg, cfg.random_patch)?;
    let chain = effects.chain_spec(preprocessed.embedded_json.as_deref())?;
    append_history(mml, &effective_patch, cfg)?;

    let phrase_dir = ensure_phrase_dir()?;
    let output_midi = phrase_dir.join("output.mid");
    let output_wav = phrase_dir.join("output.wav");
    let smf_bytes = mml_str_to_smf_bytes(&preprocessed.remaining_mml)?;
    write_smf_file(&output_midi, &smf_bytes, "MIDIファイル書き出し失敗")?;

    let patched_cfg = CoreConfig {
        output_midi: utf8_path_string(&output_midi, "出力MIDIパス")?,
        output_wav: utf8_path_string(&output_wav, "出力WAVパス")?,
        patch_path: effective_patch.clone(),
        ..cfg.clone()
    };
    let patch_display = patch_display_for_render(effective_patch.as_deref(), cfg);
    let inputs = prepare_render_inputs(&smf_bytes, patched_cfg, options, chain)?;
    Ok(PreparedPhraseRender {
        inputs,
        output_wav,
        patch_display,
    })
}

fn prepare_cache_render(
    mml: &str,
    cfg: &CoreConfig,
    options: RenderOptions,
    effects: RenderEffects,
) -> Result<PreparedCacheRender> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let effective_patch =
        resolve_effective_patch(preprocessed.embedded_json.as_deref(), cfg, false)?;
    let chain = effects.chain_spec(preprocessed.embedded_json.as_deref())?;

    let smf_bytes = mml_str_to_smf_bytes(&preprocessed.remaining_mml)?;
    let daw_dir = ensure_daw_dir()?;
    let output_midi = daw_dir.join("daw_cache.mid");
    let output_wav = daw_dir.join("daw_cache.wav");
    write_smf_file(&output_midi, &smf_bytes, "daw_cache.mid 書き出し失敗")?;

    let patched_cfg = CoreConfig {
        output_midi: utf8_path_string(&output_midi, "DAW MIDIキャッシュパス")?,
        output_wav: utf8_path_string(&output_wav, "DAW WAVキャッシュパス")?,
        patch_path: effective_patch,
        random_patch: false,
        ..cfg.clone()
    };
    let inputs = prepare_render_inputs(&smf_bytes, patched_cfg, options, chain)?;
    Ok(PreparedCacheRender { inputs, output_wav })
}

fn write_smf_file(path: &std::path::Path, smf_bytes: &[u8], label: &str) -> Result<()> {
    std::fs::write(path, smf_bytes)
        .map_err(|e| anyhow::anyhow!("{} ({}): {}", label, path.display(), e))
}

fn utf8_path_string(path: &std::path::Path, label: &str) -> Result<String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("{}が非UTF-8です: {}", label, path.display()))
}

/// MML文字列（JSON除去済み）→ SMFバイト列
pub fn mml_str_to_smf_bytes(mml: &str) -> Result<Vec<u8>> {
    raw_mml_to_smf_bytes_with_options(mml, smf_conversion_options())
}

fn smf_conversion_options() -> SmfConversionOptions {
    SmfConversionOptions {
        use_drum_channel_for_128: false,
    }
}

/// MML文字列 → SMFバイト列（外部公開用、JSON込みのMMLを受け取る）
#[allow(dead_code)]
pub fn mml_to_smf_bytes(mml: &str) -> Result<Vec<u8>> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    mml_str_to_smf_bytes(&preprocessed.remaining_mml)
}

/// realtime play 用の MML 前処理結果。
#[derive(Debug)]
pub struct PreparedRealtimePlay {
    pub smf_bytes: Vec<u8>,
    /// 解決済みパッチのパス。None は Init Saw（初期 state）。
    pub patch_path: Option<String>,
}

/// MML（JSON込み）→ SMFバイト列 + 解決済みパッチパス。
/// パッチの優先順位は offline render と同じ（MML先頭JSON → random_patch → config patch_path）。
/// patch_history.txt への追記や中間ファイルの生成は行わない。
pub fn prepare_realtime_play(mml: &str, cfg: &CoreConfig) -> Result<PreparedRealtimePlay> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let patch_path =
        resolve_effective_patch(preprocessed.embedded_json.as_deref(), cfg, cfg.random_patch)?;
    let smf_bytes = mml_str_to_smf_bytes(&preprocessed.remaining_mml)?;
    Ok(PreparedRealtimePlay {
        smf_bytes,
        patch_path,
    })
}

#[cfg(test)]
mod tests;
