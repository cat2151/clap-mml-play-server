//! preset 置き場の走査。plugin ごとに「値」の作り方が違う。
//!
//! - Surge XT Effects: `fx_presets/` からの相対パス（区切りは `/`）。表示は拡張子なし。
//!   1 ファイル 1 件。読めないファイルと未対応の effect 種別は載せない
//! - TONE3000: ファイル名が uuid で読めないので、preset 内の `name` を値にする。
//!   同名が 2 件あっても両方載せ、引くときに曖昧エラーにする
//! - Voyage Voyage: Presets root からの相対パスから拡張子を除いた物（区切りは `/`）
//! - Dragonfly Reverb / Shu: preset は host 側の組み込み表。表の preset 名を値にし、ファイルは走査しない

use std::path::{Path, PathBuf};

use anyhow::Result;
use plugin_presets::effect_preset::{relative_display, PresetValue};

use super::{AudioEffectPluginInfo, AudioEffectPreset};
use crate::dragonfly_preset::{builtin_preset_values, dragonfly_plugin};
use crate::shu_preset::{shu_preset_values, SHU_PLUGIN_ID};
use crate::surge_fx_preset::{surge_fx_value, SRGFX_EXTENSION, SURGE_FX_PLUGIN_ID};
use crate::tone3000_preset::{tone3000_value, T3K_PRESET_EXTENSION, TONE3000_PLUGIN_ID};
use crate::voyage_voyage_preset::{voyage_voyage_value, PST_EXTENSION, VOYAGE_VOYAGE_PLUGIN_ID};

/// `(preset_root, path, plugin_name)` から値を作る。読めない・未対応ならエラー。
type DescribePreset = fn(&Path, &Path, &str) -> Result<PresetValue>;

pub(super) fn scan_presets(
    plugin: &AudioEffectPluginInfo,
    presets: &mut Vec<AudioEffectPreset>,
    skipped: &mut Vec<String>,
) {
    if let Some(dragonfly) = dragonfly_plugin(&plugin.plugin_id) {
        push_builtin_presets(plugin, builtin_preset_values(dragonfly), presets);
        return;
    }
    if plugin.plugin_id == SHU_PLUGIN_ID {
        push_builtin_presets(plugin, shu_preset_values(), presets);
        return;
    }
    let (extension, describe): (&str, DescribePreset) = match plugin.plugin_id.as_str() {
        SURGE_FX_PLUGIN_ID => (SRGFX_EXTENSION, surge_fx_value),
        TONE3000_PLUGIN_ID => (T3K_PRESET_EXTENSION, tone3000_value),
        VOYAGE_VOYAGE_PLUGIN_ID => (PST_EXTENSION, voyage_voyage_value),
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
    values: Vec<PresetValue>,
    presets: &mut Vec<AudioEffectPreset>,
) {
    for PresetValue {
        value,
        shown,
        category,
        kind,
    } in values
    {
        presets.push(AudioEffectPreset {
            plugin: plugin.key.clone(),
            json_key: plugin.json_key.clone(),
            value,
            display: format!("{}: {shown}", plugin.name),
            name: shown,
            category,
            kind,
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
