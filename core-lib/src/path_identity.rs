//! 同じファイルを指すパスかどうかの判定。
//!
//! std だけで書き、`host` feature 無しでも使えるようにしてある（cat-smf-editor が
//! プラグインのパス比較だけのために、重いホスト側の依存を引き込まないため）。

use std::path::Path;

pub fn same_path(left: &str, right: &str) -> bool {
    path_key(left) == path_key(right)
}

/// 区切り（`/` と `\`）と、Windows では大文字小文字の違いを吸収した比較キー。
/// 実在するパスは `canonicalize` した形で比べる。
pub fn path_key(path: &str) -> String {
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
