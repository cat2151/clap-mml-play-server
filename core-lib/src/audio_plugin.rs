//! Plugin-neutral catalog information shared with UI clients.
//!
//! Concrete plugin detection stays in this module.  Callers route with
//! [`PluginKey`] and consume normalized patch metadata instead of branching on
//! plugin IDs, file names, or preset extensions.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    patch_list::MergedPatches,
    patch_tag::{patch_form_of_path, split_display, wire_from_display},
    read_vvp_header,
    six_sines::read_six_sines_voicing,
    PatchVoicing,
};
use cmrt_server_config::{
    patch_form_of, patch_tag_of, u_he_product_of, PatchBase, PatchForm, SURGE_XT_PLUGIN_ID,
};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PluginKey(String);

impl PluginKey {
    pub fn from_identity(plugin_id: Option<&str>, plugin_path: &str) -> Self {
        if let Some(id) = plugin_id.map(str::trim).filter(|id| !id.is_empty()) {
            return Self(format!("id:{id}"));
        }
        let path = Path::new(plugin_path.trim());
        let normalized = std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .replace('\\', "/");
        let normalized = if cfg!(windows) {
            normalized.to_ascii_lowercase()
        } else {
            normalized
        };
        Self(format!("path:{normalized}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PatchRef {
    pub plugin: PluginKey,
    pub display: String,
}

impl PatchRef {
    /// MML・履歴・config へ書く patch 文字列。表示名から作る経路はこれ 1 つに限る。
    pub fn wire(&self) -> String {
        wire_from_display(&self.display)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginVoicingSource {
    ExternalLookup,
    CatalogMetadata,
    AssumePoly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatchVoicingHint {
    Known { voicing: PatchVoicing },
    ExternalLookup { key: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PatchSortMetadata {
    pub category: String,
    pub source_rank: u8,
    pub vendor: String,
    pub category_rest: String,
    pub path_rest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AudioPatch {
    pub reference: PatchRef,
    pub normalized_display: String,
    /// Patch selector で表示・検索できる、adapter が解釈済みのカテゴリ。
    ///
    /// plugin 固有の命名規則は server 側に閉じ、client はこの値の由来を判定しない。
    #[serde(default)]
    pub selector_category: Option<String>,
    pub sort: PatchSortMetadata,
    pub voicing: PatchVoicingHint,
    /// 同じ音を鳴らす patch をこの 1 件へまとめたときの件数と名前。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged: Option<MergedPatches>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioPluginInfo {
    pub key: PluginKey,
    pub name: String,
    pub plugin_path: String,
    pub plugin_id: Option<String>,
    pub patch_base: PatchBase,
    patch_form: PatchForm,
    /// この plugin の音色を指す patch タグ（拡張子を共有する plugin を分ける）。
    patch_tag: &'static str,
}

impl AudioPluginInfo {
    pub fn new(
        name: impl Into<String>,
        plugin_path: impl Into<String>,
        plugin_id: Option<String>,
        patch_base: impl Into<PatchBase>,
    ) -> Self {
        let plugin_path = plugin_path.into();
        Self {
            key: PluginKey::from_identity(plugin_id.as_deref(), &plugin_path),
            name: name.into(),
            patch_form: patch_form_of(plugin_id.as_deref(), &plugin_path),
            patch_tag: patch_tag_of(plugin_id.as_deref(), &plugin_path),
            plugin_path,
            plugin_id,
            patch_base: patch_base.into(),
        }
    }

    pub fn voicing_source(&self) -> PluginVoicingSource {
        plugin_voicing_source(self.plugin_id.as_deref(), &self.plugin_path)
    }

    pub fn describe_patch(&self, display: &str, absolute_path: Option<&Path>) -> AudioPatch {
        let inferred_path = (self.patch_base != PatchBase::None)
            .then(|| PathBuf::from(self.patch_base.resolve(split_display(display).0)));
        describe_patch(self, display, absolute_path.or(inferred_path.as_deref()))
    }

    /// 既存catalogのpatchへ、plugin固有I/Oなしでselector用カテゴリを補完する。
    pub fn selector_category(&self, display: &str) -> Option<String> {
        let sort = patch_sort_metadata(display);
        selector_category(self, display, &sort)
    }

    /// 音色そのものが reverb などの effect を内蔵しているか。
    ///
    /// false の plugin（Dexed・sforzando・Floe・Six Sines・TyrellN6・JC303）は音色が dry なので、client は試聴時に
    /// effect を足してよい。未知の plugin は true 側へ倒す（勝手に effect を足させない）。
    /// render 済み wav は音が完成しているので true。`.h2p` は製品ごとに違う
    /// （Zebralette3 は reverb・delay を内蔵する）。
    pub fn has_builtin_effects(&self) -> bool {
        match self.patch_form {
            PatchForm::Cartridge
            | PatchForm::Sfz
            | PatchForm::FloePreset
            | PatchForm::SixSines
            | PatchForm::Jc303Preset => false,
            PatchForm::H2p => u_he_product_of(self.plugin_id.as_deref(), &self.plugin_path)
                .is_none_or(|product| product.builtin_effects),
            PatchForm::Vvp | PatchForm::StateFile | PatchForm::CacheWav => true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteError {
    Unsupported { patch: String },
    Ambiguous { patch: String, plugins: Vec<String> },
    PluginMissing { key: PluginKey },
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { patch } => {
                write!(f, "この音色を読めるプラグインがありません: '{patch}'")
            }
            Self::Ambiguous { patch, plugins } => write!(
                f,
                "音色のプラグインを一意に決められません: '{patch}' ({})",
                plugins.join(", ")
            ),
            Self::PluginMissing { key } => {
                write!(f, "音色が要求するプラグインがありません: {key}")
            }
        }
    }
}

impl std::error::Error for RouteError {}

#[derive(Clone, Debug, Default)]
pub struct AudioPluginCatalog {
    plugins: Vec<AudioPluginInfo>,
}

impl AudioPluginCatalog {
    pub fn new(plugins: Vec<AudioPluginInfo>) -> Self {
        Self { plugins }
    }

    pub fn plugins(&self) -> &[AudioPluginInfo] {
        &self.plugins
    }

    pub fn plugin(&self, key: &PluginKey) -> Result<&AudioPluginInfo, RouteError> {
        self.plugins
            .iter()
            .find(|plugin| &plugin.key == key)
            .ok_or_else(|| RouteError::PluginMissing { key: key.clone() })
    }

    /// 表示名（空タグの `:` を落とした形）から、その音色を鳴らす plugin を引く。
    pub fn route_patch(&self, patch: &str) -> Result<&AudioPluginInfo, RouteError> {
        let (path, tag) = split_display(patch);
        let Some(form) = patch_form_of_path(path) else {
            return Err(RouteError::Unsupported {
                patch: patch.to_string(),
            });
        };
        let matches = self
            .plugins
            .iter()
            .filter(|plugin| plugin.patch_form == form && plugin.patch_tag == tag)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Err(RouteError::Unsupported {
                patch: patch.to_string(),
            }),
            [plugin] => Ok(plugin),
            _ => Err(RouteError::Ambiguous {
                patch: patch.to_string(),
                plugins: matches.iter().map(|plugin| plugin.name.clone()).collect(),
            }),
        }
    }

    pub fn route_ref(&self, patch: &PatchRef) -> Result<&AudioPluginInfo, RouteError> {
        self.plugin(&patch.plugin)
    }
}

pub fn plugin_voicing_source(plugin_id: Option<&str>, plugin_path: &str) -> PluginVoicingSource {
    if is_surge(plugin_id, plugin_path) {
        PluginVoicingSource::ExternalLookup
    } else if matches!(
        patch_form_of(plugin_id, plugin_path),
        PatchForm::Vvp | PatchForm::SixSines
    ) {
        PluginVoicingSource::CatalogMetadata
    } else {
        PluginVoicingSource::AssumePoly
    }
}

/// Compatibility lookup keys for a persisted display patch name.
///
/// Adapter-specific legacy prefixes remain server-owned; clients simply try
/// the returned opaque keys in order.
pub fn patch_lookup_candidates(normalized: &str) -> Vec<String> {
    const LEGACY_PREFIXES: [&str; 2] = ["patches_factory", "patches_3rdparty"];
    let mut candidates = vec![normalized.to_string()];
    if !LEGACY_PREFIXES
        .iter()
        .any(|prefix| normalized == *prefix || normalized.starts_with(&format!("{prefix}/")))
    {
        candidates.extend(
            LEGACY_PREFIXES
                .iter()
                .map(|prefix| format!("{prefix}/{normalized}")),
        );
    }
    candidates
}

fn describe_patch(
    plugin: &AudioPluginInfo,
    display: &str,
    absolute_path: Option<&Path>,
) -> AudioPatch {
    let voicing = match plugin.voicing_source() {
        PluginVoicingSource::ExternalLookup => PatchVoicingHint::ExternalLookup {
            key: display.to_string(),
        },
        PluginVoicingSource::CatalogMetadata if plugin.patch_form == PatchForm::SixSines => {
            PatchVoicingHint::Known {
                voicing: absolute_path.map_or(PatchVoicing::Unknown, read_six_sines_voicing),
            }
        }
        PluginVoicingSource::CatalogMetadata => {
            let voicing = absolute_path
                .and_then(|path| read_vvp_header(path).ok())
                .map_or(PatchVoicing::Unknown, |header| {
                    if header.poly {
                        PatchVoicing::Poly
                    } else {
                        PatchVoicing::Mono
                    }
                });
            PatchVoicingHint::Known { voicing }
        }
        PluginVoicingSource::AssumePoly => PatchVoicingHint::Known {
            voicing: PatchVoicing::Poly,
        },
    };
    let sort = patch_sort_metadata(display);
    AudioPatch {
        reference: PatchRef {
            plugin: plugin.key.clone(),
            display: display.to_string(),
        },
        normalized_display: display.to_lowercase(),
        selector_category: selector_category(plugin, display, &sort),
        sort,
        voicing,
        merged: None,
    }
}

fn selector_category(
    plugin: &AudioPluginInfo,
    display: &str,
    sort: &PatchSortMetadata,
) -> Option<String> {
    if plugin_presets::obxf::is_obxf(plugin.plugin_id.as_deref(), &plugin.plugin_path) {
        let (path, _) = split_display(display);
        let normalized = path.replace('\\', "/");
        // system / local / user の共通基点は drive root にもなる。インストール先の
        // ProgramData 等をカテゴリとせず、本体の Patches 以下の分類を表示する。
        let relative = normalized
            .rsplit_once("/OB-Xf/Patches/")
            .map_or(normalized.as_str(), |(_, relative)| relative);
        return relative
            .split_once('/')
            .map(|(category, _)| category.to_string());
    }
    if plugin.patch_form == PatchForm::Vvp {
        let code = vvp_category_code(display);
        return cmrt_server_config::VAPORIZER2_CATEGORY_CODES
            .iter()
            .find(|(known, _)| code.eq_ignore_ascii_case(known))
            .map(|(_, category)| (*category).to_string());
    }
    if is_surge(plugin.plugin_id.as_deref(), &plugin.plugin_path)
        && has_surge_prefix(display)
        && !sort.category_rest.is_empty()
    {
        return Some(sort.category.clone());
    }
    None
}

fn is_surge(plugin_id: Option<&str>, plugin_path: &str) -> bool {
    match plugin_id {
        Some(id) => id == SURGE_XT_PLUGIN_ID,
        None => cmrt_server_config::plugin_file_stem(plugin_path)
            .to_ascii_lowercase()
            .contains("surge"),
    }
}

mod sort_metadata;

pub use sort_metadata::patch_sort_metadata;
use sort_metadata::{has_surge_prefix, vvp_category_code};

#[cfg(test)]
mod tests;
