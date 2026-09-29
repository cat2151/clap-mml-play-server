//! Six Sines の音色ファイル（`.sxsnp`）。
//!
//! 中身は `<patch id="org.baconpaul.six-sines" …>` で始まる XML で、Six Sines の
//! `clap.state` がそのまま読む形。ここは列挙・routing が共有する path 判定と、
//! catalog 用の mono/poly の読み取りだけを持つ。

use std::path::Path;

use crate::PatchVoicing;

pub use cmrt_server_config::SIX_SINES_PLUGIN_ID;

const SIX_SINES_EXTENSION: &str = ".sxsnp";
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// 再生モード（0 = Poly / 1 = Mono）の param id。`OutputNode` の idBase 500 + 23。
const PLAY_MODE_PARAM_ID: &str = "523";

/// patch path が Six Sines の音色を指しているか。拡張子の大小文字は区別しない。
pub fn is_six_sines_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(|component| {
        component.len() > SIX_SINES_EXTENSION.len()
            && component
                .get(component.len() - SIX_SINES_EXTENSION.len()..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(SIX_SINES_EXTENSION))
    })
}

/// `.sxsnp` を読んで mono/poly を返す。読めない・再生モードの param が無いときは `Unknown`。
pub fn read_six_sines_voicing(path: &Path) -> PatchVoicing {
    std::fs::read(path).map_or(PatchVoicing::Unknown, |bytes| {
        six_sines_voicing(&String::from_utf8_lossy(&bytes))
    })
}

/// `.sxsnp` の XML から mono/poly を読む。param の並びは id 順ではないので全体を走査する。
pub fn six_sines_voicing(xml: &str) -> PatchVoicing {
    let Some(value) = param_value(xml, PLAY_MODE_PARAM_ID) else {
        return PatchVoicing::Unknown;
    };
    match value.trim().parse::<f64>() {
        Ok(v) if v >= 0.5 => PatchVoicing::Mono,
        Ok(v) if v >= 0.0 => PatchVoicing::Poly,
        _ => PatchVoicing::Unknown,
    }
}

/// `<p id="…" v="…" />` のうち、id が一致する最初の要素の `v`。
fn param_value<'a>(xml: &'a str, id: &str) -> Option<&'a str> {
    let mut rest = xml;
    while let Some(start) = rest.find("<p ") {
        let tag_start = &rest[start + 3..];
        let end = tag_start.find('>')?;
        let tag = &tag_start[..end];
        if attribute(tag, "id") == Some(id) {
            return attribute(tag, "v");
        }
        rest = &tag_start[end..];
    }
    None
}

/// タグ内の `name="value"` の value。
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    loop {
        let at = rest.find(name)?;
        let preceded_by_space = rest[..at]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        let after = rest[at + name.len()..].trim_start();
        if preceded_by_space {
            if let Some(quoted) = after.strip_prefix('=').map(str::trim_start) {
                if let Some(value) = quoted.strip_prefix('"') {
                    return value.find('"').map(|close| &value[..close]);
                }
            }
        }
        rest = &rest[at + name.len()..];
    }
}

#[cfg(test)]
mod tests;
