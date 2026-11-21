//! Build the local Surge FX parameter cache directly from the matching official
//! release. Network access belongs to catalog preparation, never playback.

mod upstream;

use crate::six_sines_factory::{FactoryFetcher, HttpFetcher};
use anyhow::{ensure, Context, Result};
use plugin_presets::surge_fx::{
    decode_layout_file, SurgeFxLayoutFile, PARAM_LAYOUT_GENERATOR_VERSION, SURGE_FX_PLUGIN_ID,
};
use std::path::Path;

/// Return a validated cache, reusing it offline when version and generator match.
/// The caller atomically publishes the returned bytes after any extra checks.
pub fn prepare_layout_cache(
    plugin_path: &str,
    cache_path: &Path,
    source_dir: &Path,
) -> Result<Vec<u8>> {
    let entry = crate::load_entry(plugin_path)?;
    let version = crate::select_descriptor(&entry, Some(SURGE_FX_PLUGIN_ID))?.version;
    prepare_for_version(
        &version,
        cache_path,
        source_dir,
        &mut HttpFetcher::default(),
    )
}

pub fn prepare_for_version(
    version: &str,
    cache_path: &Path,
    source_dir: &Path,
    fetcher: &mut impl FactoryFetcher,
) -> Result<Vec<u8>> {
    let tag = release_tag(version)?;
    if let Ok(bytes) = std::fs::read(cache_path) {
        if decode_layout_file(&bytes)
            .is_ok_and(|file| file.plugin_version == version && file.source_tag == tag)
        {
            return Ok(bytes);
        }
    }
    let mut source = upstream::Upstream::resolve(&tag, source_dir, fetcher)?;
    let layouts = source.generate()?;
    let file = SurgeFxLayoutFile {
        format_version: 2,
        generator_version: PARAM_LAYOUT_GENERATOR_VERSION,
        plugin_version: version.to_owned(),
        source_tag: tag,
        source_revision: source.revision,
        sst_effects_revision: source.sst_revision,
        layouts,
    };
    let bytes = serde_json::to_vec_pretty(&file)?;
    decode_layout_file(&bytes).context("generated Surge FX layouts are invalid")?;
    Ok(bytes)
}

fn release_tag(version: &str) -> Result<String> {
    let parts: Vec<_> = version.split('.').collect();
    ensure!(parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "Surge XT Effects の版 '{version}' に対応する公式releaseを特定できません (正式な x.y.z のみ対応)");
    Ok(format!("release_xt_{version}"))
}

#[cfg(test)]
mod tests;
