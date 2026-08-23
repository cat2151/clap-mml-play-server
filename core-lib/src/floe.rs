//! Floe の音色ファイル（`.floe-preset`）。
//!
//! 1 ファイルが Floe の full-state snapshot であり、Floe 固有 CLAP extension で
//! ロードする。ここでは、列挙・routing が共有する path 判定だけを持つ。

/// `.floe-preset` を音色置き場にする Floe の CLAP plugin ID。
pub const FLOE_PLUGIN_ID: &str = "com.floe-audio.floe";

const FLOE_PRESET_EXTENSION: &str = ".floe-preset";
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// patch path が Floe preset を指しているか。拡張子の大小文字は区別しない。
pub fn is_floe_preset_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(|component| {
        component.len() > FLOE_PRESET_EXTENSION.len()
            && component
                .get(component.len() - FLOE_PRESET_EXTENSION.len()..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(FLOE_PRESET_EXTENSION))
    })
}

#[cfg(test)]
mod tests;
