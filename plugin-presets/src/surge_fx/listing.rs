//! `fx_presets/` 配下の `.srgfx` 1 ファイルを一覧の 1 件にする。

use std::path::Path;

use anyhow::{Context, Result};

use super::{param_layout, parse_srgfx};
use crate::effect_preset::{relative_display, PresetValue};

pub const SRGFX_EXTENSION: &str = "srgfx";

/// Surge の preset フォルダ（親フォルダの相対パス）→ `(category, kind)` の表。
/// フォルダがここに無ければ、フォルダ自身を category / kind にする。
pub const SURGE_FOLDER_CLASSIFICATION: &[(&str, &str, &str)] = &[
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

/// 先頭の snapshot が読めて effect 種別が対応済みなら、相対パスを値にする。
pub fn surge_fx_value(root: &Path, path: &Path, plugin_name: &str) -> Result<PresetValue> {
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
pub fn surge_classification(value: &str, plugin_name: &str) -> (String, String) {
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

#[cfg(test)]
mod tests;
