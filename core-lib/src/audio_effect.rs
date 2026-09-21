//! instrument の後段に挿す audio effect の catalog と、chain の値型。
//!
//! [`crate::audio_plugin`] と同じ境界の考え方で、client は plugin 名や preset 形式で
//! 分岐せず、catalog が返す `(json_key, value)` を MML 先頭 JSON にそのまま書く:
//!
//! ```json
//! {"effects after instrument": [
//!    {"TONE3000 preset": "Bogner Fullstack"},
//!    {"Surge XT Effects preset": "Reverb 1/Cathedral 2", "bypass": true}
//! ]}
//! ```
//!
//! 配列の順が信号の順。各要素は「plugin を決めるキー 1 つ」＋任意の
//! [`EFFECT_STAGE_BYPASS_JSON_KEY`]。bypass が `true` の段は parse 時に chain から落ちる
//! （apply 側は無変更）。
//! [`effect_chain_spec_from_embedded_json`] がこれを [`EffectChainSpec`] へ引き直し、
//! 未知のキーや catalog に無い preset はエラーにする（黙って dry で鳴らさない）。

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::logging::emit_diagnostic;
use crate::surge_fx_preset::SURGE_FX_PLUGIN_ID;
use crate::tone3000_preset::TONE3000_PLUGIN_ID;
use crate::PluginKey;

mod scan;

/// MML 先頭 JSON で effect chain を持つキー。
pub const EFFECT_CHAIN_JSON_KEY: &str = "effects after instrument";

/// chain 要素で、その段を bypass するかを持つキー。plugin を決めるキーと共存できる
/// 唯一の追加キー。値は bool。
pub const EFFECT_STAGE_BYPASS_JSON_KEY: &str = "bypass";

/// catalog の診断行のプレフィックス。
const CATALOG_LOG_PREFIX: &str = "cmrt-effect-catalog:";

/// effect plugin 1 種別。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioEffectPluginInfo {
    pub key: PluginKey,
    /// 表示名（`display` の接頭辞にもなる）。
    pub name: String,
    pub plugin_path: String,
    pub plugin_id: String,
    /// MML 先頭 JSON の chain 要素で、この plugin を指すキー。
    pub json_key: String,
    /// factory preset を再帰的に走査する root。
    pub preset_root: PathBuf,
}

impl AudioEffectPluginInfo {
    pub fn new(
        name: impl Into<String>,
        plugin_path: impl Into<String>,
        plugin_id: impl Into<String>,
        preset_root: impl Into<PathBuf>,
    ) -> Self {
        let name = name.into();
        let plugin_path = plugin_path.into();
        let plugin_id = plugin_id.into();
        Self {
            key: PluginKey::from_identity(Some(&plugin_id), &plugin_path),
            json_key: format!("{name} preset"),
            name,
            plugin_path,
            plugin_id,
            preset_root: preset_root.into(),
        }
    }
}

/// catalog に載った preset 1 件。`json_key` / `value` をそのまま MML の JSON へ書ける。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioEffectPreset {
    pub plugin: PluginKey,
    pub json_key: String,
    pub value: String,
    /// `<plugin 名>: <value>`。
    pub display: String,
    /// 一覧に出す語（`display` から plugin 名の接頭辞を剥がした形）。
    pub name: String,
    /// 何であるかの分類の大分類（5 個程度）。
    pub category: String,
    /// 何であるかの分類の種類（category の下位、15 個程度）。
    pub kind: String,
    pub path: PathBuf,
}

impl AudioEffectPreset {
    /// chain の 1 要素として MML の JSON へ書く形。
    pub fn json_element(&self) -> serde_json::Value {
        serde_json::json!({ self.json_key.clone(): self.value })
    }
}

#[derive(Clone, Debug, Default)]
pub struct AudioEffectCatalog {
    plugins: Vec<AudioEffectPluginInfo>,
    presets: Vec<AudioEffectPreset>,
    /// 走査したが載せなかったファイルとその理由。
    skipped: Vec<String>,
}

impl AudioEffectCatalog {
    /// 組み込み既定パスから探す。plugin 本体か preset 置き場が無い plugin は載せない。
    pub fn discover() -> Self {
        let catalog = Self::scan(builtin_effect_plugins());
        for line in &catalog.skipped {
            emit_diagnostic(format!("{CATALOG_LOG_PREFIX} skipped {line}"));
        }
        catalog
    }

    /// 指定した plugin の preset 置き場を走査する。plugin 本体が無いものは載せない。
    pub fn scan(plugins: Vec<AudioEffectPluginInfo>) -> Self {
        let mut catalog = Self::default();
        for plugin in plugins {
            if !Path::new(&plugin.plugin_path).is_file() {
                continue;
            }
            scan::scan_presets(&plugin, &mut catalog.presets, &mut catalog.skipped);
            catalog.plugins.push(plugin);
        }
        catalog
    }

    /// 走査せずに組む。テストや、別プロセスから受け取った一覧を載せ直すときに使う。
    pub fn with_entries(
        plugins: Vec<AudioEffectPluginInfo>,
        presets: Vec<AudioEffectPreset>,
    ) -> Self {
        Self {
            plugins,
            presets,
            skipped: Vec::new(),
        }
    }

    pub fn plugins(&self) -> &[AudioEffectPluginInfo] {
        &self.plugins
    }

    pub fn presets(&self) -> &[AudioEffectPreset] {
        &self.presets
    }

    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// preset が持つ category の一覧。重複なし・文字列順。
    pub fn categories(&self) -> Vec<String> {
        let mut categories: Vec<String> = self
            .presets
            .iter()
            .map(|preset| preset.category.clone())
            .collect();
        categories.sort();
        categories.dedup();
        categories
    }

    /// preset が持つ kind の一覧。重複なし・文字列順。
    /// `category` が `Some` ならその category 配下の kind だけ、`None` なら全体。
    pub fn kinds_in(&self, category: Option<&str>) -> Vec<String> {
        let mut kinds: Vec<String> = self
            .presets
            .iter()
            .filter(|preset| category.is_none_or(|category| preset.category == category))
            .map(|preset| preset.kind.clone())
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }

    pub fn plugin(&self, key: &PluginKey) -> Result<&AudioEffectPluginInfo> {
        self.plugins
            .iter()
            .find(|plugin| &plugin.key == key)
            .ok_or_else(|| anyhow::anyhow!("effect plugin が catalog に無い: {key}"))
    }

    /// `(json_key, value)` で preset を 1 件に決める。無ければ・複数あればエラー。
    pub fn find(&self, json_key: &str, value: &str) -> Result<&AudioEffectPreset> {
        if !self
            .plugins
            .iter()
            .any(|plugin| plugin.json_key == json_key)
        {
            bail!(
                "effect のキー '{json_key}' はどの plugin にも当たらない（使えるキー: {}）",
                self.plugins
                    .iter()
                    .map(|plugin| format!("'{}'", plugin.json_key))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let matches: Vec<&AudioEffectPreset> = self
            .presets
            .iter()
            .filter(|preset| preset.json_key == json_key && preset.value == value)
            .collect();
        match matches.as_slice() {
            [] => bail!("effect preset が catalog に無い: {{\"{json_key}\": \"{value}\"}}"),
            [preset] => Ok(preset),
            _ => bail!(
                "effect preset を一意に決められない: {{\"{json_key}\": \"{value}\"}} ({})",
                matches
                    .iter()
                    .map(|preset| preset.path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// 組み込み既定パスの effect plugin。preset 置き場が分からない OS では空。
/// 実在チェックはしない（[`AudioEffectCatalog::scan`] が行う）。
pub fn builtin_effect_plugins() -> Vec<AudioEffectPluginInfo> {
    let mut plugins = Vec::new();
    if let Some(root) = cmrt_server_config::default_tone3000_preset_root() {
        plugins.push(AudioEffectPluginInfo::new(
            "TONE3000",
            cmrt_server_config::default_tone3000_plugin_path(),
            TONE3000_PLUGIN_ID,
            root,
        ));
    }
    if let Some(root) = cmrt_server_config::default_surge_fx_preset_root() {
        plugins.push(AudioEffectPluginInfo::new(
            "Surge XT Effects",
            cmrt_server_config::default_surge_fx_plugin_path(),
            SURGE_FX_PLUGIN_ID,
            root,
        ));
    }
    plugins
}

/// preset ファイルの置き場。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PresetLocation {
    pub path: PathBuf,
    /// エラー文と表示のための `<plugin 名>: <value>`。
    pub display: String,
}

/// chain の 1 段。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectStageSpec {
    pub plugin: PluginKey,
    pub preset: PresetLocation,
}

/// instrument の後段に直列で挿す effect の列。先頭が instrument 直後。
///
/// MML から独立した値型で、JSON からの引き直しは
/// [`effect_chain_spec_from_embedded_json`] が行う。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectChainSpec(pub Vec<EffectStageSpec>);

impl EffectChainSpec {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn stages(&self) -> &[EffectStageSpec] {
        &self.0
    }
}

impl fmt::Display for EffectChainSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self
            .0
            .iter()
            .map(|stage| stage.preset.display.as_str())
            .collect();
        write!(f, "[{}]", names.join(" -> "))
    }
}

/// MML 先頭 JSON が chain を持つか（[`EFFECT_CHAIN_JSON_KEY`] があるか）。
pub fn embedded_json_has_effect_chain(embedded_json: Option<&str>) -> bool {
    embedded_json
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .is_some_and(|value| value.get(EFFECT_CHAIN_JSON_KEY).is_some())
}

/// MML 先頭 JSON から chain を引く。キーが無ければ空。
///
/// 配列でない・要素がキー 1 つのオブジェクトでない・未知のキー・catalog に無い preset は
/// すべてエラー。
pub fn effect_chain_spec_from_embedded_json(
    json: &serde_json::Value,
    catalog: &AudioEffectCatalog,
) -> Result<EffectChainSpec> {
    let Some(chain) = json.get(EFFECT_CHAIN_JSON_KEY) else {
        return Ok(EffectChainSpec::default());
    };
    let elements = chain
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("'{EFFECT_CHAIN_JSON_KEY}' は配列でなければならない"))?;
    let mut stages = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        let stage = stage_from_element(element, catalog)
            .with_context(|| format!("'{EFFECT_CHAIN_JSON_KEY}' の {} 番目", index + 1))?;
        if let Some(stage) = stage {
            stages.push(stage);
        }
    }
    Ok(EffectChainSpec(stages))
}

/// 要素を 1 段へ引き直す。bypass の段は `Ok(None)`（chain から落とす）。
fn stage_from_element(
    element: &serde_json::Value,
    catalog: &AudioEffectCatalog,
) -> Result<Option<EffectStageSpec>> {
    let object = element
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("要素はキー 1 つのオブジェクトでなければならない"))?;
    let mut bypass = false;
    let mut plugin_entries: Vec<(&String, &serde_json::Value)> = Vec::new();
    for (key, value) in object {
        if key == EFFECT_STAGE_BYPASS_JSON_KEY {
            bypass = value.as_bool().ok_or_else(|| {
                anyhow::anyhow!("'{EFFECT_STAGE_BYPASS_JSON_KEY}' の値は bool でなければならない")
            })?;
        } else {
            plugin_entries.push((key, value));
        }
    }
    let (json_key, value) = match plugin_entries.as_slice() {
        [(json_key, value)] => (*json_key, *value),
        _ => bail!(
            "要素は '{EFFECT_STAGE_BYPASS_JSON_KEY}' を除いてキー 1 つのオブジェクトでなければならない（キー {} 個）",
            plugin_entries.len()
        ),
    };
    if bypass {
        return Ok(None);
    }
    let value = value
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("'{json_key}' の値は文字列でなければならない"))?;
    let preset = catalog.find(json_key, value)?;
    Ok(Some(EffectStageSpec {
        plugin: preset.plugin.clone(),
        preset: PresetLocation {
            path: preset.path.clone(),
            display: preset.display.clone(),
        },
    }))
}

#[cfg(test)]
mod tests;
