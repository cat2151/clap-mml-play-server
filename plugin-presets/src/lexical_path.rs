//! 音色ファイルのパスを、置き場と比べられる表記にそろえる。
//!
//! `std::fs::canonicalize` は使わない。symlink を解くと置き場（ARIA の user bank root など）の外の
//! パスになって置き場からの相対名を作れず、Mount Manager に登録されていないボリューム
//! （VRAMDISK など）では OS error 1005 で失敗するため。`dunce::canonicalize` も中身は同じ呼び出し。

use std::path::{Path, PathBuf};

/// file system を見ずに、`\\?\` を外して絶対パスにする。存在は確かめない。
///
/// `..` は Windows では畳まれるが、Unix では残る。包含判定では `..` の残ったパスを拒否すること。
pub fn lexical_absolute(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    std::path::absolute(dunce::simplified(path.as_ref()))
}

#[cfg(test)]
mod tests;
