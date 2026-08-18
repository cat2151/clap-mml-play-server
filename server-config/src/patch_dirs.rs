//! 設定から「どのディレクトリに音色があるか」を導く。
//!
//! `patches_dirs` は複数書けるが、`CoreConfig.patches_dir` は 1 本しか持てない。
//! 共通の親ディレクトリへ畳んで渡す。

use std::path::{Path, PathBuf};

/// 空文字だけの要素を落とした `patches_dirs`。
pub fn configured_patch_dirs(patches_dirs: Option<&[String]>) -> Vec<String> {
    patches_dirs
        .unwrap_or_default()
        .iter()
        .filter(|dir| !dir.trim().is_empty())
        .cloned()
        .collect()
}

/// `CoreConfig.patches_dir` へ渡す 1 本のディレクトリ。
pub fn patch_root_dir(patches_dirs: Option<&[String]>) -> Option<String> {
    shared_patch_root_dir(&configured_patch_dirs(patches_dirs))
}

/// 与えられたディレクトリ群をすべて含む最も深い親ディレクトリ。
///
/// 1 つも無い場合と、共通の親が根まで登っても見つからない場合は `None`。
pub fn shared_patch_root_dir(dirs: &[String]) -> Option<String> {
    let mut dir_paths = dirs.iter().map(PathBuf::from);
    let mut common = dir_paths.next()?;
    for dir in dir_paths {
        while !Path::new(&dir).starts_with(&common) {
            if !common.pop() {
                return None;
            }
        }
    }
    if common.as_os_str().is_empty() {
        return None;
    }
    Some(common.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
