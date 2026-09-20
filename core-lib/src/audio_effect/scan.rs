//! preset 置き場の走査。plugin ごとに「値」の作り方が違う。
//!
//! - Surge XT Effects: `fx_presets/` からの相対パス（区切りは `/`）。表示は拡張子なし。
//!   1 ファイル 1 件。読めないファイルと未対応の effect 種別は載せない
//! - TONE3000: ファイル名が uuid で読めないので、preset 内の `name` を値にする。
//!   同名が 2 件あっても両方載せ、引くときに曖昧エラーにする

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{AudioEffectPluginInfo, AudioEffectPreset};
use crate::surge_fx_preset::{param_layout, parse_srgfx, SURGE_FX_PLUGIN_ID};
use crate::tone3000_preset::{parse_t3k_preset, TONE3000_PLUGIN_ID};

const SRGFX_EXTENSION: &str = "srgfx";
const T3K_PRESET_EXTENSION: &str = "t3kpreset";

/// JSON に書く値と、一覧に出す文字列。
struct PresetValue {
    value: String,
    shown: String,
}

/// `(preset_root, path)` から値を作る。読めない・未対応ならエラー。
type DescribePreset = fn(&Path, &Path) -> Result<PresetValue>;

pub(super) fn scan_presets(
    plugin: &AudioEffectPluginInfo,
    presets: &mut Vec<AudioEffectPreset>,
    skipped: &mut Vec<String>,
) {
    let (extension, describe): (&str, DescribePreset) = match plugin.plugin_id.as_str() {
        SURGE_FX_PLUGIN_ID => (SRGFX_EXTENSION, surge_fx_value),
        TONE3000_PLUGIN_ID => (T3K_PRESET_EXTENSION, tone3000_value),
        other => {
            skipped.push(format!(
                "{}: plugin_id '{other}' の preset 形式を知らない",
                plugin.name
            ));
            return;
        }
    };
    let mut files = Vec::new();
    collect_files(&plugin.preset_root, extension, &mut files);
    files.sort();
    for path in files {
        let relative = relative_display(&plugin.preset_root, &path);
        match describe(&plugin.preset_root, &path) {
            Ok(PresetValue { value, shown }) => presets.push(AudioEffectPreset {
                plugin: plugin.key.clone(),
                json_key: plugin.json_key.clone(),
                display: format!("{}: {shown}", plugin.name),
                value,
                path,
            }),
            Err(error) => skipped.push(format!("{}: {relative}: {error:#}", plugin.name)),
        }
    }
}

fn collect_files(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, extension, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            out.push(path);
        }
    }
}

/// root からの相対パスを `/` 区切りで返す。root の外なら絶対パスのまま。
fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 先頭の snapshot が読めて effect 種別が対応済みなら、相対パスを値にする。
fn surge_fx_value(root: &Path, path: &Path) -> Result<PresetValue> {
    let xml = std::fs::read_to_string(path).context("読めない")?;
    let snapshot = parse_srgfx(&xml)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("snapshot が無い"))?;
    if param_layout(snapshot.fx_type).is_none() {
        anyhow::bail!("effect 種別 {} は未対応", snapshot.fx_type);
    }
    let value = relative_display(root, path);
    let shown = value
        .strip_suffix(&format!(".{SRGFX_EXTENSION}"))
        .unwrap_or(&value)
        .to_string();
    Ok(PresetValue { value, shown })
}

/// preset 内の `name` を値にする（ファイル名は uuid で読めない）。
fn tone3000_value(_root: &Path, path: &Path) -> Result<PresetValue> {
    let bytes = std::fs::read(path).context("読めない")?;
    let name = parse_t3k_preset(&bytes)?.name;
    Ok(PresetValue {
        shown: name.clone(),
        value: name,
    })
}
