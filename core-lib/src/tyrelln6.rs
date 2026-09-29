//! TyrellN6 の音色ファイル（`.h2p`）。
//!
//! 中身は `#AM=TyrellN6` を含む u-he のテキスト形式（途中に NUL を含む）で、TyrellN6 の
//! `clap.state` がそのまま読む形。ここは列挙・routing が共有する path 判定だけを持つ。
//! `.h2p` は他の u-he plugin も使う拡張子だが、送り先は TyrellN6 に限る。

pub use cmrt_server_config::TYRELLN6_PLUGIN_ID;

const TYRELLN6_EXTENSION: &str = ".h2p";
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// patch path が TyrellN6 の音色を指しているか。拡張子の大小文字は区別しない。
pub fn is_tyrelln6_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(|component| {
        component.len() > TYRELLN6_EXTENSION.len()
            && component
                .get(component.len() - TYRELLN6_EXTENSION.len()..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(TYRELLN6_EXTENSION))
    })
}

#[cfg(test)]
mod tests;
