//! config では選べない、固定の既定プラグイン。
//!
//! 再生・rendering の既存コードはトップレベルの `plugin_path` / `plugin_id` /
//! `patches_dirs` を読む。この module は `[plugins."Surge XT"]` と組み込み値を解決し、
//! その既存 runtime view へ焼き込むための境界を持つ。

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};

use crate::{merged_plugin_profiles, PluginProfile};

pub const PRIMARY_PLUGIN_PROFILE_NAME: &str = "Surge XT";

const RETIRED_TOP_LEVEL_PLUGIN_KEYS: &[&str] = &[
    "active_plugin",
    "plugin_path",
    "plugin_id",
    "patches_dirs",
    "chord_patch_categories",
    "bass_patch_categories",
    "arpeggio_patch_categories",
    "drum_patch_categories",
    "kick_patch_keywords",
    "snare_patch_keywords",
    "hihat_patch_keywords",
];

/// 廃止済みのトップレベル plugin 設定を、未知キーとして黙って無視させない。
pub fn reject_retired_top_level_plugin_keys(text: &str) -> Result<()> {
    let table: toml::Table =
        toml::from_str(text).context("config.toml のトップレベル設定を確認できません")?;
    let found = RETIRED_TOP_LEVEL_PLUGIN_KEYS
        .iter()
        .copied()
        .filter(|key| table.contains_key(*key))
        .collect::<Vec<_>>();
    if found.is_empty() {
        return Ok(());
    }

    anyhow::bail!(
        "廃止されたトップレベルのプラグイン設定があります: {}。\
         active_plugin と用途別Role設定は削除し、plugin_path / plugin_id / patches_dirs だけを \
         [plugins.\"Surge XT\"] へ移動してください",
        found.join(", ")
    )
}

/// 組み込み Surge XT に config の `[plugins."Surge XT"]` override を重ねる。
pub fn resolve_primary_plugin_profile(
    from_config: &BTreeMap<String, PluginProfile>,
) -> Result<PluginProfile> {
    let profile = merged_plugin_profiles(from_config)
        .remove(PRIMARY_PLUGIN_PROFILE_NAME)
        .expect("Surge XT は組み込みプロファイルに必ず存在する");
    if profile.plugin_path.trim().is_empty() {
        anyhow::bail!(
            "固定の既定プラグイン Surge XT の plugin_path が空です。\
             [plugins.\"Surge XT\"] に plugin_path を書いてください"
        );
    }
    Ok(profile)
}

#[cfg(test)]
mod tests;
