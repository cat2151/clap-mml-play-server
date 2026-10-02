//! effect の preset 1 件を一覧に載せるときの値と分類。

use std::path::Path;

/// JSON に書く値と、一覧に出す文字列・分類。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresetValue {
    pub value: String,
    pub shown: String,
    pub category: String,
    pub kind: String,
}

/// root からの相対パスを `/` 区切りで返す。root の外なら絶対パスのまま。
pub fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
