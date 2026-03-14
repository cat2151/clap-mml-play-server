//! パッチリスト取得
//!
//! patches_dir 以下を再帰的に walk して .fxp ファイルを列挙する。

use anyhow::Result;
use std::path::{Path, PathBuf};

/// patches_dir 以下の .fxp ファイルをすべて列挙して返す。
/// 戻り値は絶対パス。
pub fn collect_patches(patches_dir: &str) -> Result<Vec<PathBuf>> {
    let mut list = Vec::new();
    visit_dir(Path::new(patches_dir), &mut list)?;
    list.sort();
    Ok(list)
}

fn visit_dir(dir: &Path, list: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("ディレクトリを読めない {}: {}", dir.display(), e))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_dir(&path, list)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("fxp") {
            list.push(path);
        }
    }
    Ok(())
}

/// パッチの絶対パスを「カテゴリ/ファイル名.fxp」形式に変換する。
/// patches_dir が `C:\ProgramData\Surge XT\patches_factory` のとき、
/// `Pads/Pad 1.fxp` のような形式になる。
pub fn to_relative(patches_dir: &str, abs_path: &Path) -> String {
    let base = Path::new(patches_dir);
    abs_path
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs_path.to_string_lossy().into_owned())
}

#[cfg(test)]
#[path = "patch_list_tests.rs"]
mod tests;
