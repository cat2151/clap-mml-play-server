//! Plugin-neutral catalog information shared with UI clients.
//!
//! Concrete plugin detection stays in this module.  Callers route with
//! [`PluginKey`] and consume normalized patch metadata instead of branching on
//! plugin IDs, file names, or preset extensions.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    is_cartridge_patch_path, is_floe_preset_path, is_sfz_patch_path, is_vvp_patch_path,
    read_vvp_header, PatchVoicing,
};
use cmrt_server_config::{patch_form_of, PatchForm, SURGE_XT_PLUGIN_ID};

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioPluginInfo {
    pub key: PluginKey,
    pub name: String,
    pub plugin_path: String,
    pub plugin_id: Option<String>,
    pub patch_root: Option<String>,
    patch_form: PatchForm,
}

impl AudioPluginInfo {
    pub fn new(
        name: impl Into<String>,
        plugin_path: impl Into<String>,
        plugin_id: Option<String>,
        patch_root: Option<String>,
    ) -> Self {
        let plugin_path = plugin_path.into();
        Self {
            key: PluginKey::from_identity(plugin_id.as_deref(), &plugin_path),
            name: name.into(),
            patch_form: patch_form_of(plugin_id.as_deref(), &plugin_path),
            plugin_path,
            plugin_id,
            patch_root,
        }
    }

    pub fn voicing_source(&self) -> PluginVoicingSource {
        plugin_voicing_source(self.plugin_id.as_deref(), &self.plugin_path)
    }

    pub fn describe_patch(&self, display: &str, absolute_path: Option<&Path>) -> AudioPatch {
        let inferred_path = self
            .patch_root
            .as_deref()
            .map(|root| Path::new(root).join(display));
        describe_patch(self, display, absolute_path.or(inferred_path.as_deref()))
    }

    /// 既存catalogのpatchへ、plugin固有I/Oなしでselector用カテゴリを補完する。
    pub fn selector_category(&self, display: &str) -> Option<String> {
        let sort = patch_sort_metadata(display);
        selector_category(self, display, &sort)
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

    pub fn route_patch(&self, patch: &str) -> Result<&AudioPluginInfo, RouteError> {
        let form = patch_form_of_path(patch);
        let matches = self
            .plugins
            .iter()
            .filter(|plugin| plugin.patch_form == form)
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
    } else if patch_form_of(plugin_id, plugin_path) == PatchForm::Vvp {
        PluginVoicingSource::CatalogMetadata
    } else {
        PluginVoicingSource::AssumePoly
    }
}

pub fn patch_sort_metadata(path: &str) -> PatchSortMetadata {
    let path = path.trim_matches(['/', '\\']);
    if is_vvp_patch_path(path) {
        return vvp_sort_metadata(path);
    }
    if has_surge_prefix(path) {
        return surge_sort_metadata(path);
    }
    first_segment_metadata(path)
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
    }
}

fn selector_category(
    plugin: &AudioPluginInfo,
    display: &str,
    sort: &PatchSortMetadata,
) -> Option<String> {
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

pub(crate) fn patch_form_of_path(patch: &str) -> PatchForm {
    if is_cartridge_patch_path(patch) {
        PatchForm::Cartridge
    } else if is_sfz_patch_path(patch) {
        PatchForm::Sfz
    } else if is_floe_preset_path(patch) {
        PatchForm::FloePreset
    } else if is_vvp_patch_path(patch) {
        PatchForm::Vvp
    } else {
        PatchForm::StateFile
    }
}

fn is_surge(plugin_id: Option<&str>, plugin_path: &str) -> bool {
    match plugin_id {
        Some(id) => id == SURGE_XT_PLUGIN_ID,
        None => cmrt_server_config::plugin_file_stem(plugin_path)
            .to_ascii_lowercase()
            .contains("surge"),
    }
}

fn has_surge_prefix(path: &str) -> bool {
    path.starts_with("patches_factory/") || path.starts_with("patches_3rdparty/")
}

fn split_first(path: &str) -> (&str, &str) {
    path.split_once('/').unwrap_or((path, ""))
}

fn first_segment_metadata(path: &str) -> PatchSortMetadata {
    let (category, rest) = split_first(path);
    PatchSortMetadata {
        category: category.to_string(),
        source_rank: 0,
        vendor: String::new(),
        category_rest: rest.to_string(),
        path_rest: path.to_string(),
    }
}

fn surge_sort_metadata(path: &str) -> PatchSortMetadata {
    if let Some(rest) = path.strip_prefix("patches_factory/") {
        let (category, rest) = split_first(rest);
        return PatchSortMetadata {
            category: category.to_string(),
            source_rank: 0,
            vendor: String::new(),
            category_rest: rest.to_string(),
            path_rest: rest_path(path, "patches_factory/"),
        };
    }
    let rest = path.strip_prefix("patches_3rdparty/").unwrap_or(path);
    let (first, rest) = split_first(rest);
    let (category, vendor, category_rest) = if rest.is_empty() || !rest.contains('/') {
        (first, "", rest)
    } else {
        let (category, category_rest) = split_first(rest);
        (category, first, category_rest)
    };
    PatchSortMetadata {
        category: category.to_string(),
        source_rank: 1,
        vendor: vendor.to_string(),
        category_rest: category_rest.to_string(),
        path_rest: rest_path(path, "patches_3rdparty/"),
    }
}

fn rest_path(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_string()
}

fn vvp_sort_metadata(path: &str) -> PatchSortMetadata {
    let code = vvp_category_code(path);
    let category = cmrt_server_config::VAPORIZER2_CATEGORY_CODES
        .iter()
        .find(|(known, _)| code.eq_ignore_ascii_case(known))
        .map_or(code, |(_, name)| *name);
    PatchSortMetadata {
        category: category.to_string(),
        source_rank: 0,
        vendor: String::new(),
        category_rest: path.to_string(),
        path_rest: path.to_string(),
    }
}

fn vvp_category_code(path: &str) -> &str {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file_name
        .rfind('.')
        .filter(|dot| *dot > 0)
        .map_or(file_name, |dot| &file_name[..dot]);
    let end = stem
        .char_indices()
        .nth(2)
        .map_or(stem.len(), |(end, _)| end);
    &stem[..end]
}

#[cfg(test)]
mod tests;
