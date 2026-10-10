//! Strict preparation of the seven catalog instrument formats.
use std::path::Path;

use super::{prepare_clap_patch_state, prepare_native_clap_patch_state, PatchStateError};
use crate::{
    DEXED_PLUGIN_ID, FLOE_PLUGIN_ID, SFORZANDO_PLUGIN_ID, SIX_SINES_PLUGIN_ID, SURGE_XT_PLUGIN_ID,
    TYRELLN6_PLUGIN_ID, VAPORIZER2_PLUGIN_ID,
};

pub fn supports_catalog_clap_plugin(id: &str) -> bool {
    matches!(
        id,
        SURGE_XT_PLUGIN_ID
            | DEXED_PLUGIN_ID
            | FLOE_PLUGIN_ID
            | SFORZANDO_PLUGIN_ID
            | SIX_SINES_PLUGIN_ID
            | TYRELLN6_PLUGIN_ID
            | VAPORIZER2_PLUGIN_ID
    )
}

/// Prepare one resolved catalog reference. See the native preparation API for
/// its thread and cross-host serialization contract. Pure formats create no host.
/// Dexed's program suffix is part of `patch` and must not be discarded.
pub fn prepare_catalog_clap_patch_state(
    id: &str,
    bundle: &Path,
    patch: &Path,
) -> Result<Vec<u8>, PatchStateError> {
    if id == SURGE_XT_PLUGIN_ID {
        return prepare_clap_patch_state(id, patch);
    }
    if matches!(id, DEXED_PLUGIN_ID | FLOE_PLUGIN_ID | SFORZANDO_PLUGIN_ID) {
        return prepare_native_clap_patch_state(id, bundle, patch);
    }
    let extension = match id {
        SIX_SINES_PLUGIN_ID => "sxsnp",
        TYRELLN6_PLUGIN_ID => "h2p",
        VAPORIZER2_PLUGIN_ID => "vvp",
        _ => {
            return Err(PatchStateError::UnsupportedPlugin {
                plugin_id: id.into(),
            })
        }
    };
    if !patch.is_absolute() {
        return Err(PatchStateError::InvalidPath {
            path: patch.into(),
            reason: "an absolute patch path is required".into(),
        });
    }
    if !patch
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        return Err(PatchStateError::UnsupportedFormat {
            path: patch.into(),
            reason: format!("{id} requires .{extension}"),
        });
    }
    let bytes = std::fs::read(patch).map_err(|source| PatchStateError::Io {
        path: patch.into(),
        source,
    })?;
    let invalid = |reason: String| PatchStateError::InvalidData {
        path: patch.into(),
        reason,
    };
    if id == TYRELLN6_PLUGIN_ID {
        if !bytes
            .split(|b| *b == b'\n')
            .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == b"#AM=TyrellN6")
        {
            return Err(invalid("missing TyrellN6 model marker".into()));
        }
        return Ok(bytes);
    }
    let xml = xmltree::Element::parse(replace_control_char_refs(&bytes).as_slice())
        .map_err(|e| invalid(format!("invalid patch XML: {e}")))?;
    if id == SIX_SINES_PLUGIN_ID {
        if xml.name != "patch"
            || xml.attributes.get("id").map(String::as_str) != Some(id)
            || xml.get_child("params").is_none()
        {
            return Err(invalid("Six Sines patch identity or params missing".into()));
        }
        Ok(bytes)
    } else {
        if xml.name != "VASTvaporizer2" {
            return Err(invalid("missing Vaporizer2 XML root".into()));
        }
        crate::parse_vvp_header(&bytes).map_err(|e| invalid(format!("{e:#}")))?;
        Ok(crate::vvp_state_blob(&bytes))
    }
}

/// Plugins write control characters such as `&#29;` (JUCE does) that XML 1.0
/// forbids and they read back. Replace them with `?` for the structural check only.
fn replace_control_char_refs(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(at) = rest.windows(2).position(|w| w == b"&#") {
        out.extend_from_slice(&rest[..at]);
        rest = &rest[at..];
        let end = rest.iter().take(12).position(|&b| b == b';');
        let code = end.and_then(|end| {
            let digits = std::str::from_utf8(&rest[2..end]).ok()?;
            match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => digits.parse().ok(),
            }
        });
        match (end, code) {
            (Some(end), Some(code)) if code < 0x20 && !matches!(code, 0x9 | 0xA | 0xD) => {
                out.push(b'?');
                rest = &rest[end + 1..];
            }
            _ => {
                out.extend_from_slice(b"&#");
                rest = &rest[2..];
            }
        }
    }
    out.extend_from_slice(rest);
    out
}

#[cfg(test)]
mod tests;
