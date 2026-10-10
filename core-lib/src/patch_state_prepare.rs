//! Host-independent preparation of the supported Surge XT CLAP patch format.

use std::{
    error::Error,
    fmt, io,
    path::{Path, PathBuf},
};

use crate::SURGE_XT_PLUGIN_ID;

mod native;
pub use native::prepare_native_clap_patch_state;
mod catalog;
pub use catalog::{prepare_catalog_clap_patch_state, supports_catalog_clap_plugin};

/// A patch could not be prepared. Paths and plugin IDs are retained for callers.
#[derive(Debug)]
pub enum PatchStateError {
    UnsupportedPlugin { plugin_id: String },
    InvalidPath { path: PathBuf, reason: String },
    UnsupportedFormat { path: PathBuf, reason: String },
    Io { path: PathBuf, source: io::Error },
    InvalidData { path: PathBuf, reason: String },
}

impl fmt::Display for PatchStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlugin { plugin_id } => {
                write!(f, "unsupported CLAP plugin: {plugin_id}")
            }
            Self::InvalidPath { path, reason }
            | Self::UnsupportedFormat { path, reason }
            | Self::InvalidData { path, reason } => write!(f, "{}: {reason}", path.display()),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl Error for PatchStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Read and convert a Surge XT FXP into owned CLAP state bytes.
///
/// Only the exact CLAP ID `org.surge-synth-team.surge-xt` and an absolute `.fxp`
/// path are supported (the extension is ASCII case-insensitive). These are
/// checked before reading. Relative paths are not resolved against the CWD;
/// canonicalization is not required. Missing files and access errors are [`PatchStateError::Io`].
///
/// The supported container has a 60-byte `CcnK` / `FPCh` / `cjs3` header,
/// followed by a `sub3` chunk. Its declared chunk size must match the file,
/// and the XML and six wavetable lengths must remain within the chunk.
/// Surge's zero `byteSize` is accepted. No truncation, magic search, or raw
/// fallback is performed. The result is the chunk, not the FXP container.
///
/// This function only reads and converts data. It creates no host, plugin,
/// audio device, UI, or server, and can run on a worker thread. XML semantics
/// and DSP validity belong to the plugin: successful preparation does not
/// imply successful state loading or audible output.
pub fn prepare_clap_patch_state(
    plugin_id: &str,
    absolute_patch_path: &Path,
) -> Result<Vec<u8>, PatchStateError> {
    if plugin_id != SURGE_XT_PLUGIN_ID {
        return Err(PatchStateError::UnsupportedPlugin {
            plugin_id: plugin_id.into(),
        });
    }
    let path = absolute_patch_path;
    if !path.is_absolute() {
        return Err(PatchStateError::InvalidPath {
            path: path.into(),
            reason: "an absolute patch path is required".into(),
        });
    }
    if !is_fxp(path) {
        return Err(PatchStateError::UnsupportedFormat {
            path: path.into(),
            reason: "only .fxp patches are supported".into(),
        });
    }
    let raw = std::fs::read(path).map_err(|source| PatchStateError::Io {
        path: path.into(),
        source,
    })?;
    decode_surge_fxp(&raw, path).map(<[u8]>::to_vec)
}

pub(crate) fn is_fxp(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("fxp"))
}

fn decode_surge_fxp<'a>(raw: &'a [u8], path: &Path) -> Result<&'a [u8], PatchStateError> {
    let invalid = |reason: &str| PatchStateError::InvalidData {
        path: path.into(),
        reason: reason.into(),
    };
    let header = raw
        .get(..60)
        .ok_or_else(|| invalid("truncated FXP header"))?;
    if header.get(..4) != Some(b"CcnK")
        || header.get(8..12) != Some(b"FPCh")
        || header.get(16..20) != Some(b"cjs3")
    {
        return Err(invalid("invalid Surge FXP magic or ID"));
    }
    let chunk_len =
        read_length(header, 56, true).ok_or_else(|| invalid("missing FXP chunk length"))?;
    let end = 60usize
        .checked_add(chunk_len)
        .ok_or_else(|| invalid("FXP chunk length overflow"))?;
    if end != raw.len() {
        return Err(invalid("FXP chunk length does not match file length"));
    }
    let chunk = raw
        .get(60..end)
        .ok_or_else(|| invalid("invalid FXP chunk boundary"))?;
    let sub_header = chunk
        .get(..32)
        .ok_or_else(|| invalid("truncated sub3 header"))?;
    if sub_header.get(..4) != Some(b"sub3") {
        return Err(invalid("missing sub3 magic at offset 60"));
    }
    // sub3 stores the XML length and six wavetable byte lengths in little endian.
    let mut payload_end = 32usize;
    for offset in (4..32).step_by(4) {
        let length = read_length(sub_header, offset, false)
            .ok_or_else(|| invalid("missing sub3 payload length"))?;
        payload_end = payload_end
            .checked_add(length)
            .ok_or_else(|| invalid("sub3 payload length overflow"))?;
        if payload_end > chunk.len() {
            return Err(invalid("sub3 XML or wavetable length exceeds chunk"));
        }
    }
    Ok(chunk)
}

fn read_length(bytes: &[u8], offset: usize, big_endian: bool) -> Option<usize> {
    let value = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    usize::try_from(if big_endian {
        u32::from_be_bytes(value)
    } else {
        u32::from_le_bytes(value)
    })
    .ok()
}

#[cfg(test)]
mod tests;
