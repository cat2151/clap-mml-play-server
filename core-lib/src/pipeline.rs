//! MML → SMF → WAV → 再生 パイプライン

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use hound::{SampleFormat, WavSpec, WavWriter};
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};

use crate::midi::parse_smf_bytes;
use crate::patch_list::{collect_patches, to_relative};
use crate::render::render_to_memory;
use crate::CoreConfig;

use mmlabc_to_smf::{mml_preprocessor, pass1_parser, pass2_ast, pass3_events, pass4_midi};

#[path = "pipeline_dirs.rs"]
mod cmrt_dirs;
#[cfg(test)]
#[path = "pipeline_test_support.rs"]
mod test_support;

pub use cmrt_dirs::{ensure_cmrt_dir, ensure_daw_dir, ensure_phrase_dir};
#[cfg(test)]
pub(crate) use test_support::{env_lock, EnvVarGuard};

const RENDER_CHANNELS: u64 = 2;

/// レンダリング時に追加で適用する前処理。
///
/// `Millis(100)` を指定すると、MIDI イベントを 100ms 後ろへずらしてレンダリングし、
/// 生成後に先頭 100ms を切り落とす。プラグイン初期化直後の発音欠け対策を、通常再生と
/// キャッシュレンダリングの両方へ同じ形で差し込める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderPreroll {
    #[default]
    Disabled,
    Millis(u64),
    Samples(u64),
}

impl RenderPreroll {
    pub fn disabled() -> Self {
        Self::Disabled
    }

    pub fn from_millis(millis: u64) -> Self {
        Self::Millis(millis)
    }

    pub fn from_samples(samples: u64) -> Self {
        Self::Samples(samples)
    }

    fn samples(self, sample_rate: f64) -> u64 {
        match self {
            Self::Disabled => 0,
            Self::Samples(samples) => samples,
            Self::Millis(millis) => {
                let samples = sample_rate * millis as f64 / 1000.0;
                if !samples.is_finite() || samples <= 0.0 {
                    0
                } else if samples >= u64::MAX as f64 {
                    u64::MAX
                } else {
                    samples.ceil() as u64
                }
            }
        }
    }
}

/// MML レンダリングの追加オプション。
///
/// 既定値は現在の挙動を保つため preroll 無効。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderOptions {
    preroll: RenderPreroll,
}

impl RenderOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_preroll(mut self, preroll: RenderPreroll) -> Self {
        self.preroll = preroll;
        self
    }

    pub fn with_preroll_ms(self, millis: u64) -> Self {
        self.with_preroll(RenderPreroll::from_millis(millis))
    }

    pub fn preroll(&self) -> RenderPreroll {
        self.preroll
    }

    fn preroll_samples(&self, sample_rate: f64) -> u64 {
        self.preroll.samples(sample_rate)
    }
}

struct PreparedRenderInputs {
    patched_cfg: CoreConfig,
    events: Vec<crate::midi::TimedMidiEvent>,
    total_samples: u64,
    preroll_samples: u64,
}

/// MML → レンダリングのみ。再生はしない。
/// 戻り値: (サンプル列, 使用パッチ相対パス)
pub fn mml_render(mml: &str, cfg: &CoreConfig, entry: &PluginEntry) -> Result<(Vec<f32>, String)> {
    mml_render_with_options(mml, cfg, entry, RenderOptions::default())
}

/// MML → レンダリングのみ。`RenderOptions` で preroll などの追加処理を指定できる。
pub fn mml_render_with_options(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
) -> Result<(Vec<f32>, String)> {
    let prepared = prepare_phrase_render(mml, cfg, options)?;
    let patch_display = prepared.patch_display;
    let output_wav = prepared.output_wav;
    let samples = render_prepared_inputs(prepared.inputs, entry)?;
    write_wav(&samples, cfg.sample_rate as u32, &output_wav)?;
    Ok((samples, patch_display))
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
pub fn mml_render_for_cache_with_options(
    mml: &str,
    cfg: &CoreConfig,
    entry: &PluginEntry,
    options: RenderOptions,
) -> Result<Vec<f32>> {
    let prepared = prepare_cache_render(mml, cfg, options)?;
    let output_wav = prepared.output_wav;
    let samples = render_prepared_inputs(prepared.inputs, entry)?;
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
) -> Result<PreparedPhraseRender> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let effective_patch =
        resolve_effective_patch(preprocessed.embedded_json.as_deref(), cfg, cfg.random_patch)?;
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
    let inputs = prepare_render_inputs(&smf_bytes, patched_cfg, options)?;
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
) -> Result<PreparedCacheRender> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let effective_patch =
        resolve_effective_patch(preprocessed.embedded_json.as_deref(), cfg, false)?;

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
    let inputs = prepare_render_inputs(&smf_bytes, patched_cfg, options)?;
    Ok(PreparedCacheRender { inputs, output_wav })
}

fn prepare_render_inputs(
    smf_bytes: &[u8],
    patched_cfg: CoreConfig,
    options: RenderOptions,
) -> Result<PreparedRenderInputs> {
    let (events, total_samples) = parse_smf_bytes(smf_bytes, patched_cfg.sample_rate)?;
    let preroll_samples = options.preroll_samples(patched_cfg.sample_rate);
    let (events, total_samples) = apply_render_preroll(events, total_samples, preroll_samples);
    Ok(PreparedRenderInputs {
        patched_cfg,
        events,
        total_samples,
        preroll_samples,
    })
}

fn render_prepared_inputs(prepared: PreparedRenderInputs, entry: &PluginEntry) -> Result<Vec<f32>> {
    let PreparedRenderInputs {
        patched_cfg,
        events,
        total_samples,
        preroll_samples,
    } = prepared;
    let samples = render_to_memory(&patched_cfg, entry, events, total_samples)?;
    Ok(trim_render_preroll(samples, preroll_samples))
}

fn apply_render_preroll(
    events: Vec<crate::midi::TimedMidiEvent>,
    total_samples: u64,
    preroll_samples: u64,
) -> (Vec<crate::midi::TimedMidiEvent>, u64) {
    if preroll_samples == 0 {
        return (events, total_samples);
    }
    let events = events
        .into_iter()
        .map(|event| crate::midi::TimedMidiEvent {
            sample_pos: event.sample_pos.saturating_add(preroll_samples),
            message: event.message,
        })
        .collect();
    (events, total_samples.saturating_add(preroll_samples))
}

fn trim_render_preroll(samples: Vec<f32>, preroll_samples: u64) -> Vec<f32> {
    let trim_len = preroll_samples.saturating_mul(RENDER_CHANNELS);
    let trim_len = usize::try_from(trim_len).unwrap_or(usize::MAX);
    samples.into_iter().skip(trim_len).collect()
}

fn resolve_effective_patch(
    embedded_json: Option<&str>,
    cfg: &CoreConfig,
    allow_random_patch: bool,
) -> Result<Option<String>> {
    if let Some(patch) = extract_patch_from_json(embedded_json, cfg) {
        return Ok(Some(patch));
    }
    if allow_random_patch {
        return pick_random_patch(cfg);
    }
    Ok(cfg.patch_path.clone())
}

fn patch_display_for_render(effective_patch: Option<&str>, cfg: &CoreConfig) -> String {
    match effective_patch {
        Some(abs) => {
            if let Some(ref base) = cfg.patches_dir {
                to_relative(base, std::path::Path::new(abs))
            } else {
                abs.to_string()
            }
        }
        None => "(Init Saw)".to_string(),
    }
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

/// MML先頭JSONから "Surge XT patch" キーの値を取り出し、絶対パスに変換する。
fn extract_patch_from_json(json_str: Option<&str>, cfg: &CoreConfig) -> Option<String> {
    let json_str = json_str?;
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let rel = v.get("Surge XT patch")?.as_str()?;
    // patches_dir があれば絶対パスに変換、なければそのまま
    if let Some(ref base) = cfg.patches_dir {
        let abs = std::path::Path::new(base).join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        Some(abs.to_string_lossy().into_owned())
    } else {
        Some(rel.to_string())
    }
}

/// patches_dir からランダムに1つ選んで絶対パスを返す。
fn pick_random_patch(cfg: &CoreConfig) -> Result<Option<String>> {
    let dir = match &cfg.patches_dir {
        Some(d) => d,
        None => return Ok(None),
    };
    let patches = collect_patches(dir)?;
    if patches.is_empty() {
        return Ok(None);
    }
    // 簡易乱数: 現在時刻のナノ秒を使う
    let idx = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0) as usize;
        ns % patches.len()
    };
    Ok(Some(patches[idx].to_string_lossy().into_owned()))
}

/// patch_history.txt に「JSON、MML」形式で追記する。
fn append_history(mml: &str, patch: &Option<String>, cfg: &CoreConfig) -> Result<()> {
    let patch_rel = match patch {
        Some(abs) => {
            if let Some(ref base) = cfg.patches_dir {
                to_relative(base, std::path::Path::new(abs))
            } else {
                abs.clone()
            }
        }
        None => "(none)".to_string(),
    };

    // JSON部分を除いたMML本文（先頭JSONがあれば除去済みのものを使う）
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let mml_body = preprocessed.remaining_mml.trim().to_string();

    let json = format!(
        "{{\"Surge XT patch\": \"{}\"}}",
        patch_rel.replace('\\', "/")
    );
    let line = format!("{} {}\n", json, mml_body);

    use std::io::Write;
    let Some(path) =
        dirs::config_local_dir().map(|d| d.join("clap-mml-render-tui").join("patch_history.txt"))
    else {
        return Ok(()); // ディレクトリが取得できない場合はスキップ
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| anyhow::anyhow!("patch_history.txt のディレクトリ作成失敗: {}", e))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| anyhow::anyhow!("patch_history.txt を開けない: {}", e))?;
    file.write_all(line.as_bytes())
        .map_err(|e| anyhow::anyhow!("patch_history.txt への書き込み失敗: {}", e))?;
    Ok(())
}

/// MML文字列（JSON除去済み）→ SMFバイト列
pub fn mml_str_to_smf_bytes(mml: &str) -> Result<Vec<u8>> {
    let cmrt_dir = ensure_cmrt_dir()?;
    // process_pass{1,2,3} は &str を受け取るため、PathBuf から &str への変換が必要。
    // 非UTF-8パスは明示的にエラーとして扱い、サイレントなパス破壊を防ぐ。
    let pass1 = cmrt_dir.join("pass1_tokens.json");
    let pass2 = cmrt_dir.join("pass2_ast.json");
    let pass3 = cmrt_dir.join("pass3_events.json");
    let pass1_str = pass1
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("パスが非UTF-8です: {}", pass1.display()))?;
    let pass2_str = pass2
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("パスが非UTF-8です: {}", pass2.display()))?;
    let pass3_str = pass3
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("パスが非UTF-8です: {}", pass3.display()))?;
    let tokens = pass1_parser::process_pass1(mml, pass1_str)?;
    let ast = pass2_ast::process_pass2(&tokens, pass2_str)?;
    let events = pass3_events::process_pass3(&ast, pass3_str, false)?;
    let smf_bytes = pass4_midi::events_to_midi(&events)?;
    Ok(smf_bytes)
}

/// MML文字列 → SMFバイト列（外部公開用、JSON込みのMMLを受け取る）
#[allow(dead_code)]
pub fn mml_to_smf_bytes(mml: &str) -> Result<Vec<u8>> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    mml_str_to_smf_bytes(&preprocessed.remaining_mml)
}

/// Vec<f32>（インターリーブステレオ）を WAVファイルに書き出す
pub fn write_wav(
    samples: &[f32],
    sample_rate: u32,
    path: impl AsRef<std::path::Path>,
) -> Result<()> {
    let path = path.as_ref();
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut wav = WavWriter::create(path, spec)
        .map_err(|e| anyhow::anyhow!("WAVファイル作成失敗 ({}): {}", path.display(), e))?;
    for &s in samples {
        wav.write_sample(s)
            .map_err(|e| anyhow::anyhow!("WAV書き込み失敗: {}", e))?;
    }
    wav.finalize()?;
    Ok(())
}

/// Vec<f32>（インターリーブステレオ）を rodio で再生する
pub fn play_samples(samples: Vec<f32>, sample_rate: u32) -> Result<()> {
    let (_stream, stream_handle) = OutputStream::try_default()
        .map_err(|e| anyhow::anyhow!("オーディオ出力の初期化失敗: {}", e))?;
    let sink =
        Sink::try_new(&stream_handle).map_err(|e| anyhow::anyhow!("Sink の作成失敗: {}", e))?;
    let source = SamplesBuffer::new(2, sample_rate, samples);
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
