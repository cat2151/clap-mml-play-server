//! sforzando の CLAP preset-discovery と config の音色置き場を合成する。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use clack_extensions::preset_discovery::{self, prelude::*};
use clack_host::prelude::{HostError, HostInfo, PluginEntry};

type CachedDiscovery = Result<Vec<PathBuf>, String>;
type DiscoveryCache = OnceLock<Mutex<HashMap<String, CachedDiscovery>>>;

/// discovery と config を解決した結果。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PatchDirResolution {
    /// 実在確認と canonical path での重複排除を終えた音色置き場。
    pub dirs: Vec<String>,
    /// config に書かれていたが実在しなかった場所。
    pub configured_missing: Vec<String>,
    /// discovery が利用できなかった理由。config 由来の場所だけで継続した場合も残す。
    pub discovery_error: Option<String>,
}

/// sforzando が宣言する `.sfz` の filesystem location と config の場所を合成する。
pub fn resolve_sforzando_patch_dirs(
    plugin_path: &str,
    configured: Option<&[String]>,
) -> PatchDirResolution {
    resolve_patch_dirs(configured, cached_sforzando_patch_dirs(plugin_path))
}

fn cached_sforzando_patch_dirs(plugin_path: &str) -> anyhow::Result<Vec<PathBuf>> {
    static DISCOVERED: DiscoveryCache = OnceLock::new();
    let cache = DISCOVERED.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(plugin_path)
        .cloned()
    {
        return cached.map_err(anyhow::Error::msg);
    }
    let discovered =
        discover_sforzando_patch_dirs(plugin_path).map_err(|error| format!("{error:#}"));
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(plugin_path.to_string(), discovered.clone());
    discovered.map_err(anyhow::Error::msg)
}

fn discover_sforzando_patch_dirs(plugin_path: &str) -> anyhow::Result<Vec<PathBuf>> {
    // SAFETY: CLAP entry のロードは clack の契約どおり entry の生存期間内だけ使う。
    let entry = unsafe { PluginEntry::load(plugin_path) }
        .map_err(|error| anyhow::anyhow!("CLAP entry をロードできない: {error:?}"))?;
    let factory = entry
        .get_factory::<PresetDiscoveryFactory>()
        .ok_or_else(|| anyhow::anyhow!("clap.preset-discovery-factory/2 がない"))?;
    let host_info = HostInfo::new(
        "clap-mml-render",
        "clap-mml-render",
        "https://github.com/cat2151/clap-mml-play-server",
        env!("CARGO_PKG_VERSION"),
    )?;

    let mut locations = Vec::new();
    let mut provider_errors = Vec::new();
    let mut provider_count = 0usize;
    let mut sfz_provider_count = 0usize;
    let mut declared_filetypes = Vec::new();
    let mut declared_locations = Vec::new();
    for descriptor in factory.provider_descriptors() {
        provider_count += 1;
        let Some(provider_id) = descriptor.id() else {
            provider_errors.push("provider ID がない".to_string());
            continue;
        };
        let provider_name = descriptor
            .name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| provider_id.to_string_lossy().into_owned());
        let provider =
            Provider::instantiate(DiscoveryIndexer::default(), &entry, provider_id, &host_info);
        let provider = match provider {
            Ok(provider) => provider,
            Err(error) => {
                provider_errors.push(format!("{provider_name}: {error}"));
                continue;
            }
        };
        let indexer = provider.indexer();
        declared_filetypes.extend(indexer.filetypes.iter().cloned());
        declared_locations.extend(
            indexer
                .filesystem_locations
                .iter()
                .map(|path| path.to_string_lossy().into_owned()),
        );
        if !indexer.supports_sfz {
            continue;
        }
        sfz_provider_count += 1;
        locations.extend(indexer.filesystem_locations.iter().cloned());
    }

    if !locations.is_empty() {
        return Ok(locations);
    }
    if provider_count == 0 {
        anyhow::bail!("preset-discovery provider がない");
    }
    if sfz_provider_count == 0 {
        let detail = if provider_errors.is_empty() {
            String::new()
        } else {
            format!(" ({})", provider_errors.join(" / "))
        };
        let filetypes = declared_filetypes.join(", ");
        let locations = declared_locations.join(", ");
        anyhow::bail!(
            ".sfz filetype を宣言する provider がない (宣言: filetypes=[{filetypes}] locations=[{locations}]){detail}"
        );
    }
    anyhow::bail!(".sfz provider が filesystem location を宣言しなかった")
}

#[derive(Default)]
struct DiscoveryIndexer {
    supports_sfz: bool,
    filetypes: Vec<String>,
    filesystem_locations: Vec<PathBuf>,
}

impl IndexerImpl for DiscoveryIndexer {
    fn declare_filetype(
        &mut self,
        file_type: preset_discovery::preset_data::FileType,
    ) -> Result<(), HostError> {
        let extension = file_type
            .file_extension
            .map(|extension| extension.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.supports_sfz |= extension
            .trim_start_matches('.')
            .eq_ignore_ascii_case("sfz");
        self.filetypes.push(format!(
            "{}:*.{}",
            file_type.name.to_string_lossy(),
            extension
        ));
        Ok(())
    }

    fn declare_location(
        &mut self,
        location: preset_discovery::preset_data::LocationInfo,
    ) -> Result<(), HostError> {
        if let Some(path) = location.location.file_path() {
            self.filesystem_locations
                .push(PathBuf::from(path.to_string_lossy().into_owned()));
        }
        Ok(())
    }

    fn declare_soundpack(
        &mut self,
        _soundpack: preset_discovery::preset_data::Soundpack,
    ) -> Result<(), HostError> {
        Ok(())
    }
}

fn resolve_patch_dirs(
    configured: Option<&[String]>,
    discovered: anyhow::Result<Vec<PathBuf>>,
) -> PatchDirResolution {
    let configured = configured.unwrap_or_default();
    let mut candidates = configured.iter().map(PathBuf::from).collect::<Vec<_>>();
    let mut configured_missing = configured
        .iter()
        .filter(|dir| !Path::new(dir.as_str()).is_dir())
        .cloned()
        .collect::<Vec<_>>();
    let discovery_error = match discovered {
        Ok(discovered) => {
            let any_existing = discovered.iter().any(|path| path.is_dir());
            candidates.extend(discovered);
            (!any_existing)
                .then(|| "preset-discovery が宣言した filesystem location が実在しない".to_string())
        }
        Err(error) => Some(format!("{error:#}")),
    };

    let mut seen = HashSet::new();
    let mut dirs = Vec::new();
    for candidate in candidates {
        if !candidate.is_dir() {
            continue;
        }
        let canonical = match std::fs::canonicalize(&candidate) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let key = canonical_key(&canonical);
        if seen.insert(key) {
            dirs.push(canonical.to_string_lossy().into_owned());
        }
    }
    configured_missing.sort();
    configured_missing.dedup();
    PatchDirResolution {
        dirs,
        configured_missing,
        discovery_error,
    }
}

fn canonical_key(path: &Path) -> String {
    let key = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

#[cfg(test)]
mod tests;
