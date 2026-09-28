//! patch 文字列（display）と音色ファイルの絶対パスを行き来するときの基点。
//!
//! display は保存済みの MML 先頭 JSON / history / DAW セルが指す永続 ID なので、
//! display ⇄ 絶対パスの規則はここ 1 か所に置く。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::shared_patch_root_dir;

/// 1 プラグインぶんの音色の基点。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PatchBase {
    /// 相対化しない。display は絶対パスそのもの。
    #[default]
    None,
    /// 全置き場を含む 1 本の親ディレクトリ。display はここからの相対。
    Shared(String),
    /// 置き場ごとに基点を持つ。display は各置き場の**親**からの相対なので、
    /// 先頭要素が置き場のフォルダ名になる（例 `Free Sounds/Programs/Piano.sfz`）。
    ///
    /// 置き場の決まり方が互いに独立していて、共通の親に意味が無いプラグイン（Sforzando）用。
    /// 置き場どうしが別ドライブにあっても相対化できる。
    PerRoot(Vec<String>),
}

impl PatchBase {
    /// 置き場群の共通の親を基点にする。共通の親が無ければ [`PatchBase::None`]。
    pub fn shared(dirs: &[String]) -> Self {
        shared_patch_root_dir(dirs).map_or(Self::None, Self::Shared)
    }

    /// 置き場ごとに基点を持つ。置き場が無ければ [`PatchBase::None`]。
    pub fn per_root(dirs: &[String]) -> Self {
        if dirs.is_empty() {
            Self::None
        } else {
            Self::PerRoot(dirs.to_vec())
        }
    }

    /// display を絶対パスへ直す。絶対パスはそのまま返す。
    ///
    /// [`PatchBase::PerRoot`] では、先頭要素とフォルダ名が一致する置き場の親へ繋ぐ。
    /// どの置き場とも一致しなければ入力のまま返す（別の置き場を推測しない）。
    pub fn resolve(&self, patch: &str) -> String {
        if Path::new(patch).is_absolute() {
            return patch.to_string();
        }
        match self {
            Self::None => patch.to_string(),
            Self::Shared(base) => Path::new(base).join(patch).to_string_lossy().into_owned(),
            Self::PerRoot(roots) => {
                resolve_per_root(roots, patch).unwrap_or_else(|| patch.to_string())
            }
        }
    }

    /// 絶対パスを display へ直す。区切りは `/`。基点の外なら絶対パスのまま返す。
    pub fn display(&self, path: &Path) -> String {
        let relative = match self {
            Self::None => None,
            Self::Shared(base) => path.strip_prefix(base).ok(),
            Self::PerRoot(roots) => roots.iter().find_map(|root| {
                path.strip_prefix(root).ok()?;
                path.strip_prefix(display_anchor(Path::new(root))).ok()
            }),
        };
        match relative {
            Some(relative) => relative.to_string_lossy().replace('\\', "/"),
            None => path.to_string_lossy().into_owned(),
        }
    }

    /// 置き場を 1 本のディレクトリとして走査できるならそのディレクトリ（ランダム選択用）。
    pub fn scan_dir(&self) -> Option<&str> {
        match self {
            Self::Shared(base) => Some(base),
            Self::None | Self::PerRoot(_) => None,
        }
    }
}

impl From<Option<String>> for PatchBase {
    /// 1 本のディレクトリ（または無し）を基点にする。
    fn from(dir: Option<String>) -> Self {
        dir.map_or(Self::None, Self::Shared)
    }
}

/// display の先頭要素を置き場のフォルダ名にするための、相対化の起点（置き場の親）。
fn display_anchor(root: &Path) -> &Path {
    root.parent().unwrap_or(root)
}

fn resolve_per_root(roots: &[String], patch: &str) -> Option<String> {
    let mut parts = patch.split(['/', '\\']).filter(|part| !part.is_empty());
    let first = parts.next()?;
    let root = roots.iter().map(Path::new).find(|root| {
        root.file_name()
            .is_some_and(|name| name.to_string_lossy() == first)
    })?;
    let mut path = PathBuf::from(root);
    path.extend(parts);
    Some(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
