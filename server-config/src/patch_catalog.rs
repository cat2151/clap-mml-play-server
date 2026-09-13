//! Plugin-neutral loadable patch catalog facade.
//!
//! Callers depend on paths and diagnostics. Vendor adapters stay behind this dispatch point.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::{configured_patch_dirs, patch_form_of, sforzando_programs, PatchForm};

/// Plugin-neutral result consumed by catalog clients.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PatchCatalogResolution {
    /// Existing, canonical scan roots.
    pub dirs: Vec<String>,
    /// `Some` means the adapter has already restricted the list to loadable files.
    /// `None` means the caller should use its normal directory scanner.
    pub resolved_patches: Option<Vec<PathBuf>>,
    pub configured_missing: Vec<String>,
    /// No usable program source could be resolved.
    pub source_error: Option<String>,
    /// Partial failures and excluded-file counts. Usable programs remain available.
    pub notices: Vec<String>,
}

/// Resolve the catalog source for any plugin profile.
///
/// Plugin-specific work is dispatched here so callers in the TUI repository do not branch on
/// Sforzando or know anything about vendor program coordinates.
pub fn resolve_patch_catalog(
    plugin_id: Option<&str>,
    plugin_path: &str,
    configured: Option<&[String]>,
) -> PatchCatalogResolution {
    if patch_form_of(plugin_id, plugin_path) == PatchForm::Sfz {
        return cached_sforzando_catalog(plugin_id, plugin_path, configured);
    }
    resolve_plain_directories(configured)
}

/// Resolve only the directory roots needed to route relative patch paths.
///
/// Unlike [`resolve_patch_catalog`], this never recursively enumerates patch files or parses every
/// installed-bank manifest. Realtime servers use it when the persistent catalog-source cache is
/// absent or stale, so a missing cache does not turn every server start into a full catalog scan.
pub fn resolve_patch_catalog_roots(
    plugin_id: Option<&str>,
    plugin_path: &str,
    configured: Option<&[String]>,
) -> PatchCatalogResolution {
    if patch_form_of(plugin_id, plugin_path) == PatchForm::Sfz {
        return sforzando_programs::resolve_roots(configured, Path::new(plugin_path).exists());
    }
    resolve_plain_directories(configured)
}

fn cached_sforzando_catalog(
    plugin_id: Option<&str>,
    plugin_path: &str,
    configured: Option<&[String]>,
) -> PatchCatalogResolution {
    static CACHE: OnceLock<Mutex<HashMap<String, PatchCatalogResolution>>> = OnceLock::new();
    let key = format!(
        "{}\u{1f}{}\u{1f}{}",
        plugin_id.unwrap_or_default(),
        plugin_path,
        configured.unwrap_or_default().join("\u{1e}")
    );
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(resolution) = cache.get(&key).cloned() {
        return resolution;
    }
    let resolution =
        sforzando_programs::resolve_catalog(configured, Path::new(plugin_path).exists());
    cache.insert(key, resolution.clone());
    resolution
}

pub(super) fn resolve_plain_directories(configured: Option<&[String]>) -> PatchCatalogResolution {
    let configured = configured_patch_dirs(configured);
    let mut configured_missing = Vec::new();
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    for candidate in configured {
        let path = Path::new(&candidate);
        if !path.is_dir() {
            configured_missing.push(candidate);
            continue;
        }
        if let Ok(canonical) = std::fs::canonicalize(path) {
            let key = canonical.to_string_lossy().into_owned();
            let key = if cfg!(windows) {
                key.to_lowercase()
            } else {
                key
            };
            if seen.insert(key) {
                dirs.push(canonical.to_string_lossy().into_owned());
            }
        }
    }
    configured_missing.sort();
    configured_missing.dedup();
    PatchCatalogResolution {
        dirs,
        configured_missing,
        ..PatchCatalogResolution::default()
    }
}

#[cfg(test)]
mod tests;
