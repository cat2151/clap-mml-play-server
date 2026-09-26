//! preset 置き場の走査。plugin ごとに「値」の作り方が違う。
//!
//! - Surge XT Effects: `fx_presets/` からの相対パス（区切りは `/`）。表示は拡張子なし。
//!   1 ファイル 1 件。読めないファイルと未対応の effect 種別は載せない
//! - TONE3000: ファイル名が uuid で読めないので、preset 内の `name` を値にする。
//!   同名が 2 件あっても両方載せ、引くときに曖昧エラーにする
//! - Dragonfly Reverb: preset は本体に組み込み。表の preset 名を値にし、ファイルは走査しない

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{AudioEffectPluginInfo, AudioEffectPreset};
use crate::dragonfly_preset::{dragonfly_plugin, DragonflyPlugin};
use crate::surge_fx_preset::{param_layout, parse_srgfx, SURGE_FX_PLUGIN_ID};
use crate::tone3000_preset::{parse_t3k_preset, TONE3000_PLUGIN_ID};

const SRGFX_EXTENSION: &str = "srgfx";
const T3K_PRESET_EXTENSION: &str = "t3kpreset";

/// Surge の preset フォルダ（親フォルダの相対パス）→ `(category, kind)` の表。
/// フォルダがここに無ければ、フォルダ自身を category / kind にする。
pub(super) const SURGE_FOLDER_CLASSIFICATION: &[(&str, &str, &str)] = &[
    ("EQ", "Filter / EQ", "EQ"),
    ("Graphic EQ", "Filter / EQ", "EQ"),
    ("Airwindows/Filter", "Filter / EQ", "Filter"),
    ("Resonator", "Filter / EQ", "Filter"),
    ("Combulator", "Filter / EQ", "Filter"),
    ("Distortion", "Distortion / Saturation", "Distortion"),
    ("CHOW", "Distortion / Saturation", "Distortion"),
    ("Neuron", "Distortion / Saturation", "Distortion"),
    ("Exciter", "Distortion / Saturation", "Saturation / Exciter"),
    ("Bonsai", "Distortion / Saturation", "Saturation / Exciter"),
    (
        "Airwindows/Saturation And More",
        "Distortion / Saturation",
        "Saturation / Exciter",
    ),
    ("Tape", "Distortion / Saturation", "Tape"),
    ("Airwindows/Tape", "Distortion / Saturation", "Tape"),
    ("Airwindows/Lo-Fi", "Distortion / Saturation", "LoFi"),
    ("Airwindows/Noise", "Distortion / Saturation", "LoFi"),
    ("Chorus", "Modulation", "Chorus / Ensemble"),
    ("Ensemble", "Modulation", "Chorus / Ensemble"),
    ("Rotary", "Modulation", "Rotary"),
    ("Phaser", "Modulation", "Phaser / Flanger"),
    ("Flanger", "Modulation", "Phaser / Flanger"),
    ("Ring Mod", "Modulation", "Ring Modulator"),
    ("Treemonster", "Modulation", "Ring Modulator"),
    ("Freq Shift", "Modulation", "Freq Shift"),
    ("Airwindows/Pitch", "Modulation", "Pitch / Granular"),
    ("Nimbus", "Modulation", "Pitch / Granular"),
    ("Reverb 2", "Space / Imaging", "Reverb"),
    ("Airwindows/Ambience", "Space / Imaging", "Reverb"),
    ("Reverb 1", "Space / Imaging", "Reverb"),
    ("Delay", "Space / Imaging", "Delay"),
    ("Mid-Side Tool", "Space / Imaging", "Stereo"),
    ("Airwindows/Stereo", "Space / Imaging", "Stereo"),
    ("Airwindows/Dynamics", "Dynamics", "Compressor"),
    ("Conditioner", "Dynamics", "Limiter / Clipper"),
    ("Airwindows/Clipping", "Dynamics", "Limiter / Clipper"),
];

/// Dragonfly Reverb の preset の分類（4 plugin とも reverb）。
pub(super) const DRAGONFLY_CATEGORY: &str = "Space / Imaging";
pub(super) const DRAGONFLY_KIND: &str = "Reverb";

/// TONE3000 preset の分類（種類は 1 つしか無い）。
pub(super) const TONE3000_CATEGORY: &str = "Distortion / Saturation";
pub(super) const TONE3000_KIND: &str = "Amp Simulator";

/// JSON に書く値と、一覧に出す文字列・分類。
struct PresetValue {
    value: String,
    shown: String,
    category: String,
    kind: String,
}

/// `(preset_root, path, plugin_name)` から値を作る。読めない・未対応ならエラー。
type DescribePreset = fn(&Path, &Path, &str) -> Result<PresetValue>;

pub(super) fn scan_presets(
    plugin: &AudioEffectPluginInfo,
    presets: &mut Vec<AudioEffectPreset>,
    skipped: &mut Vec<String>,
) {
    if let Some(dragonfly) = dragonfly_plugin(&plugin.plugin_id) {
        push_builtin_presets(plugin, dragonfly, presets);
        return;
    }
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
        match describe(&plugin.preset_root, &path, &plugin.name) {
            Ok(PresetValue {
                value,
                shown,
                category,
                kind,
            }) => presets.push(AudioEffectPreset {
                plugin: plugin.key.clone(),
                json_key: plugin.json_key.clone(),
                display: format!("{}: {shown}", plugin.name),
                name: shown,
                category,
                kind,
                value,
                path,
            }),
            Err(error) => skipped.push(format!("{}: {relative}: {error:#}", plugin.name)),
        }
    }
}

fn push_builtin_presets(
    plugin: &AudioEffectPluginInfo,
    dragonfly: &DragonflyPlugin,
    presets: &mut Vec<AudioEffectPreset>,
) {
    for preset in dragonfly.presets {
        presets.push(AudioEffectPreset {
            plugin: plugin.key.clone(),
            json_key: plugin.json_key.clone(),
            value: preset.name.to_string(),
            display: format!("{}: {}", plugin.name, preset.name),
            name: preset.name.to_string(),
            category: DRAGONFLY_CATEGORY.to_string(),
            kind: DRAGONFLY_KIND.to_string(),
            path: PathBuf::from(&plugin.plugin_path),
        });
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
fn surge_fx_value(root: &Path, path: &Path, plugin_name: &str) -> Result<PresetValue> {
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
    let (category, kind) = surge_classification(&value, plugin_name);
    Ok(PresetValue {
        value,
        shown,
        category,
        kind,
    })
}

/// 値の親フォルダ（最後の `/` より前）で [`SURGE_FOLDER_CLASSIFICATION`] を引く。
/// 表に無ければフォルダ自身を category / kind にする。`/` が無ければ plugin 名。
pub(super) fn surge_classification(value: &str, plugin_name: &str) -> (String, String) {
    let Some((folder, _)) = value.rsplit_once('/') else {
        return (plugin_name.to_string(), plugin_name.to_string());
    };
    match SURGE_FOLDER_CLASSIFICATION
        .iter()
        .find(|(entry_folder, _, _)| *entry_folder == folder)
    {
        Some((_, category, kind)) => (category.to_string(), kind.to_string()),
        None => (folder.to_string(), folder.to_string()),
    }
}

/// preset 内の `name` を値にする（ファイル名は uuid で読めない）。
fn tone3000_value(_root: &Path, path: &Path, _plugin_name: &str) -> Result<PresetValue> {
    let bytes = std::fs::read(path).context("読めない")?;
    let name = parse_t3k_preset(&bytes)?.name;
    Ok(PresetValue {
        shown: name.clone(),
        value: name,
        category: TONE3000_CATEGORY.to_string(),
        kind: TONE3000_KIND.to_string(),
    })
}
