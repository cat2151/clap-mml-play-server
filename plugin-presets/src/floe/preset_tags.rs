//! `.floe-preset` に保存された preset のタグ。
//!
//! タグは「u16 little endian の byte 長＋UTF-8」で並ぶ。長さ込みで照合するので、
//! `tonal percussion`（xylophone など）は `percussion` と一致しない。

use std::path::Path;

/// Floe の打楽器 preset に付くタグ。
const PERCUSSION_TAG: &str = "percussion";

/// preset に `percussion` タグが付いているか。読めないときだけ `Err` を返す。
pub fn floe_preset_is_percussion(path: &Path) -> std::io::Result<bool> {
    Ok(has_tag(&std::fs::read(path)?, PERCUSSION_TAG))
}

fn has_tag(preset: &[u8], tag: &str) -> bool {
    let Ok(len) = u16::try_from(tag.len()) else {
        return false;
    };
    let mut needle = len.to_le_bytes().to_vec();
    needle.extend_from_slice(tag.as_bytes());
    preset.windows(needle.len()).any(|window| window == needle)
}

#[cfg(test)]
mod tests;
