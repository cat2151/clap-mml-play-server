//! MML → SMF → WAV → 再生 パイプライン

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use hound::{WavSpec, WavWriter, SampleFormat};
use rodio::{OutputStream, Sink, buffer::SamplesBuffer};

use crate::config::Config;
use crate::midi::parse_smf_bytes;
use crate::patch_list::{collect_patches, to_relative};
use crate::render::render_to_memory;

use mmlabc_to_smf::{
    mml_preprocessor,
    pass1_parser,
    pass2_ast,
    pass3_events,
    pass4_midi,
};

/// MML → レンダリングのみ。再生はしない。
/// 戻り値: (サンプル列, 使用パッチ相対パス)
pub fn mml_render(mml: &str, cfg: &Config, entry: &PluginEntry) -> Result<(Vec<f32>, String)> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let json_patch = extract_patch_from_json(preprocessed.embedded_json.as_deref(), cfg);

    let effective_patch: Option<String> = if let Some(p) = json_patch {
        Some(p)
    } else if cfg.random_patch {
        pick_random_patch(cfg)?
    } else {
        cfg.patch_path.clone()
    };

    append_history(mml, &effective_patch, cfg)?;

    let smf_bytes = mml_str_to_smf_bytes(&preprocessed.remaining_mml)?;
    std::fs::write(&cfg.output_midi, &smf_bytes)
        .map_err(|e| anyhow::anyhow!("MIDIファイル書き出し失敗: {}", e))?;

    let (events, total_samples) = parse_smf_bytes(&smf_bytes, cfg.sample_rate)?;

    let patched_cfg = Config {
        plugin_path: cfg.plugin_path.clone(),
        input_midi:  cfg.input_midi.clone(),
        output_midi: cfg.output_midi.clone(),
        output_wav:  cfg.output_wav.clone(),
        sample_rate: cfg.sample_rate,
        buffer_size: cfg.buffer_size,
        patch_path:  effective_patch.clone(),
        patches_dir: cfg.patches_dir.clone(),
        random_patch: cfg.random_patch,
    };

    let samples = render_to_memory(&patched_cfg, entry, events, total_samples)?;
    write_wav(&samples, cfg.sample_rate as u32, &cfg.output_wav)?;

    let patch_display = match &effective_patch {
        Some(abs) => {
            if let Some(ref base) = cfg.patches_dir {
                to_relative(base, std::path::Path::new(abs))
            } else {
                abs.clone()
            }
        }
        None => "(Init Saw)".to_string(),
    };
    Ok((samples, patch_display))
}

/// キャッシュ構築専用の MML → レンダリング。
/// - `patch_history.txt` への追記は行わない
/// - MIDI/WAV の出力先は専用のテンポラリパス（`daw_cache.mid` / `daw_cache.wav`）を使用
///   することで通常の出力ファイルを上書きしない
/// - 呼び出し元はシリアルな単一ワーカースレッドから呼び出すこと（ファイル書き込みの
///   競合を防ぐため）
pub fn mml_render_for_cache(mml: &str, cfg: &Config, entry: &PluginEntry) -> Result<Vec<f32>> {
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let json_patch = extract_patch_from_json(preprocessed.embedded_json.as_deref(), cfg);

    let effective_patch: Option<String> = if let Some(p) = json_patch {
        Some(p)
    } else {
        cfg.patch_path.clone()
    };

    let smf_bytes = mml_str_to_smf_bytes(&preprocessed.remaining_mml)?;
    std::fs::write("daw_cache.mid", &smf_bytes)
        .map_err(|e| anyhow::anyhow!("daw_cache.mid 書き出し失敗: {}", e))?;

    let (events, total_samples) = parse_smf_bytes(&smf_bytes, cfg.sample_rate)?;

    let patched_cfg = Config {
        plugin_path: cfg.plugin_path.clone(),
        input_midi:  cfg.input_midi.clone(),
        output_midi: "daw_cache.mid".to_string(),
        output_wav:  "daw_cache.wav".to_string(),
        sample_rate: cfg.sample_rate,
        buffer_size: cfg.buffer_size,
        patch_path:  effective_patch,
        patches_dir: cfg.patches_dir.clone(),
        random_patch: false,
    };

    let samples = render_to_memory(&patched_cfg, entry, events, total_samples)?;
    write_wav(&samples, cfg.sample_rate as u32, "daw_cache.wav")?;

    Ok(samples)
}

/// MML文字列 → SMF・WAVファイル出力 + 即時再生
/// 優先順位:
///   1. MML先頭のJSON `{"Surge XT patch": "Pads/Pad 1.fxp"}` で指定されたパッチ
///   2. random_patch = true なら patches_dir からランダム選択
///   3. config.toml の patch_path
///   4. Init Saw（デフォルト）
/// 戻り値: 使用したパッチの相対パス（またはnone文字列）
pub fn mml_to_play(mml: &str, cfg: &Config, entry: &PluginEntry) -> Result<String> {
    // --- Step 1: MML先頭JSONを解析 ---
    let preprocessed = mml_preprocessor::extract_embedded_json(mml);
    let json_patch = extract_patch_from_json(preprocessed.embedded_json.as_deref(), cfg);

    // --- Step 2: 使用パッチを決定 ---
    let effective_patch: Option<String> = if let Some(p) = json_patch {
        // MML先頭JSONが最優先
        Some(p)
    } else if cfg.random_patch {
        // ランダム選択
        pick_random_patch(cfg)?
    } else {
        // config.tomlのpatch_path
        cfg.patch_path.clone()
    };

    // --- Step 3: 履歴に追記 ---
    append_history(mml, &effective_patch, cfg)?;

    // --- Step 4: MML → SMF バイト列 ---
    let smf_bytes = mml_str_to_smf_bytes(&preprocessed.remaining_mml)?;

    // --- Step 5: SMFファイル書き出し ---
    std::fs::write(&cfg.output_midi, &smf_bytes)
        .map_err(|e| anyhow::anyhow!("MIDIファイル書き出し失敗 ({}): {}", cfg.output_midi, e))?;

    // --- Step 6: SMF → イベント列 ---
    let (events, total_samples) = parse_smf_bytes(&smf_bytes, cfg.sample_rate)?;

    // --- Step 7: パッチを一時的にcfgに反映してレンダリング ---
    let patched_cfg = Config {
        plugin_path: cfg.plugin_path.clone(),
        input_midi:  cfg.input_midi.clone(),
        output_midi: cfg.output_midi.clone(),
        output_wav:  cfg.output_wav.clone(),
        sample_rate: cfg.sample_rate,
        buffer_size: cfg.buffer_size,
        patch_path:  effective_patch,
        patches_dir: cfg.patches_dir.clone(),
        random_patch: cfg.random_patch,
    };

    let samples = render_to_memory(&patched_cfg, entry, events, total_samples)?;

    // --- Step 8: WAVファイル書き出し ---
    write_wav(&samples, cfg.sample_rate as u32, &cfg.output_wav)?;

    // --- Step 9: 再生 ---
    play_samples(samples, cfg.sample_rate as u32)?;

    // 使用したパッチの相対パスを返す
    let patch_display = match &patched_cfg.patch_path {
        Some(abs) => {
            if let Some(ref base) = cfg.patches_dir {
                to_relative(base, std::path::Path::new(abs))
            } else {
                abs.clone()
            }
        }
        None => "(Init Saw)".to_string(),
    };
    Ok(patch_display)
}

/// MML先頭JSONから "Surge XT patch" キーの値を取り出し、絶対パスに変換する。
fn extract_patch_from_json(json_str: Option<&str>, cfg: &Config) -> Option<String> {
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
fn pick_random_patch(cfg: &Config) -> Result<Option<String>> {
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
fn append_history(mml: &str, patch: &Option<String>, cfg: &Config) -> Result<()> {
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

    let json = format!("{{\"Surge XT patch\": \"{}\"}}", patch_rel.replace('\\', "/"));
    let line = format!("{} {}\n", json, mml_body);

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("patch_history.txt")
        .map_err(|e| anyhow::anyhow!("patch_history.txt を開けない: {}", e))?;
    file.write_all(line.as_bytes())
        .map_err(|e| anyhow::anyhow!("patch_history.txt への書き込み失敗: {}", e))?;
    Ok(())
}

/// MML文字列（JSON除去済み）→ SMFバイト列
pub fn mml_str_to_smf_bytes(mml: &str) -> Result<Vec<u8>> {
    let tokens = pass1_parser::process_pass1(mml, "pass1_tokens.json")?;
    let ast = pass2_ast::process_pass2(&tokens, "pass2_ast.json")?;
    let events = pass3_events::process_pass3(&ast, "pass3_events.json", false)?;
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
pub fn write_wav(samples: &[f32], sample_rate: u32, path: &str) -> Result<()> {
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut wav = WavWriter::create(path, spec)
        .map_err(|e| anyhow::anyhow!("WAVファイル作成失敗 ({}): {}", path, e))?;
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
    let sink = Sink::try_new(&stream_handle)
        .map_err(|e| anyhow::anyhow!("Sink の作成失敗: {}", e))?;
    let source = SamplesBuffer::new(2, sample_rate, samples);
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
