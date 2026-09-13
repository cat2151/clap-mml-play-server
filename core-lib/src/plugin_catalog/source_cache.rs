//! `cmrt build-patch-catalog-cache` が保存した、server起動用catalog source cacheのreader。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use cmrt_server_config::PatchCatalogResolution;

const FORMAT_VERSION: u32 = 1;
const CACHE_RELATIVE_PATH: &str = "patch-catalog/sources.json";

#[derive(Deserialize)]
struct SourceCache {
    format_version: u32,
    plugins: Vec<CachedSource>,
}

#[derive(Deserialize)]
struct CachedSource {
    plugin_path: String,
    plugin_id: Option<String>,
    dirs: Vec<String>,
    #[serde(default)]
    source_notices: Vec<String>,
}

pub(super) fn load_sforzando(
    plugin_path: &str,
    current_dirs: &[String],
) -> Result<PatchCatalogResolution> {
    let path = cache_file_path().context("catalog source cacheの保存先を取得できません")?;
    load_sforzando_from(&path, plugin_path, current_dirs).with_context(|| {
        "catalog source cacheを利用できません（`cmrt build-patch-catalog-cache` を実行してください）"
    })
}

fn cache_file_path() -> Option<PathBuf> {
    cmrt_server_config::config_app_dir().map(|dir| dir.join(CACHE_RELATIVE_PATH))
}

fn load_sforzando_from(
    path: &Path,
    plugin_path: &str,
    current_dirs: &[String],
) -> Result<PatchCatalogResolution> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("catalog source cacheを読めません: {}", path.display()))?;
    let cache: SourceCache = serde_json::from_slice(&bytes)
        .with_context(|| format!("catalog source cacheが不正です: {}", path.display()))?;
    if cache.format_version != FORMAT_VERSION {
        anyhow::bail!(
            "catalog source cacheのformat versionが非対応です: expected={}, actual={}",
            FORMAT_VERSION,
            cache.format_version
        );
    }
    let source = cache
        .plugins
        .into_iter()
        .find(|source| {
            source.plugin_id.as_deref() == Some(cmrt_server_config::SFORZANDO_PLUGIN_ID)
                && same_path(&source.plugin_path, plugin_path)
        })
        .context("catalog source cacheに現在のSforzandoがありません")?;
    if !same_dirs(&source.dirs, current_dirs) {
        anyhow::bail!(
            "catalog source cacheが現在のSforzando rootと一致しません（`cmrt build-patch-catalog-cache` を実行してください）"
        );
    }
    let source_error = source
        .dirs
        .is_empty()
        .then(|| "ARIA program sourceのrootを1件も解決できない".to_string());
    Ok(PatchCatalogResolution {
        dirs: source.dirs,
        source_error,
        notices: source.source_notices,
        ..PatchCatalogResolution::default()
    })
}

fn same_dirs(left: &[String], right: &[String]) -> bool {
    let mut left = left.iter().map(|path| path_key(path)).collect::<Vec<_>>();
    let mut right = right.iter().map(|path| path_key(path)).collect::<Vec<_>>();
    left.sort();
    right.sort();
    left == right
}

fn same_path(left: &str, right: &str) -> bool {
    path_key(left) == path_key(right)
}

fn path_key(path: &str) -> String {
    let path = Path::new(path.trim());
    let key = std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

#[cfg(test)]
mod tests;
