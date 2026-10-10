//! Preparation gate for formats requiring a temporary native instance.
use std::path::Path;

use super::PatchStateError;
use crate::{parse_cartridge_patch_path, DEXED_PLUGIN_ID, FLOE_PLUGIN_ID, SFORZANDO_PLUGIN_ID};

/// Prepare an owned state using cmrt's existing Dexed, Floe or sforzando loader.
///
/// The bundle and resolved patch must be absolute. Dexed's resolved patch must
/// retain its cartridge program suffix. All native objects stay on the calling
/// thread and are destroyed before returning. Callers must serialize against
/// all instance creation in other hosts, including UAPMD.
pub fn prepare_native_clap_patch_state(
    plugin_id: &str,
    bundle: &Path,
    resolved_patch: &Path,
) -> Result<Vec<u8>, PatchStateError> {
    let invalid = |reason: &str| PatchStateError::InvalidData {
        path: resolved_patch.into(),
        reason: reason.into(),
    };
    if !matches!(
        plugin_id,
        DEXED_PLUGIN_ID | FLOE_PLUGIN_ID | SFORZANDO_PLUGIN_ID
    ) {
        return Err(PatchStateError::UnsupportedPlugin {
            plugin_id: plugin_id.into(),
        });
    }
    for path in [bundle, resolved_patch] {
        if !path.is_absolute() {
            return Err(PatchStateError::InvalidPath {
                path: path.into(),
                reason: "an absolute path is required".into(),
            });
        }
    }
    let patch = resolved_patch
        .to_str()
        .ok_or_else(|| invalid("patch path is not UTF-8"))?;
    let bundle_text = bundle
        .to_str()
        .ok_or_else(|| invalid("bundle path is not UTF-8"))?;
    let extension = resolved_patch
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let source = match plugin_id {
        DEXED_PLUGIN_ID => {
            let program =
                parse_cartridge_patch_path(patch).map_err(|e| invalid(&format!("{e:#}")))?;
            std::path::PathBuf::from(program.cartridge_path)
        }
        FLOE_PLUGIN_ID if extension.eq_ignore_ascii_case("floe-preset") => resolved_patch.into(),
        SFORZANDO_PLUGIN_ID
            if extension.eq_ignore_ascii_case("sfz") || extension.eq_ignore_ascii_case("ariax") =>
        {
            resolved_patch.into()
        }
        _ => {
            return Err(PatchStateError::UnsupportedFormat {
                path: resolved_patch.into(),
                reason: format!("patch format does not match {plugin_id}"),
            })
        }
    };
    if !bundle
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("clap"))
    {
        return Err(PatchStateError::UnsupportedFormat {
            path: bundle.into(),
            reason: "a CLAP bundle is required".into(),
        });
    }
    std::fs::metadata(&source).map_err(|source_error| PatchStateError::Io {
        path: source,
        source: source_error,
    })?;
    crate::render::prepare_native_state(bundle_text, plugin_id, patch)
        .map_err(|e| invalid(&format!("native preparation: {e:#}")))
}

#[cfg(test)]
mod tests;
