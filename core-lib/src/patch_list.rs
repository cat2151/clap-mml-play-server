//! パッチリスト取得
//!
//! patches_dir 以下を再帰的に walk して、プラグインが読める音色を列挙する。
//!
//! - Surge XT: `.fxp` 1 ファイル = 1 音色。ファイルパスがそのまま音色。
//! - Dexed: `.syx` 1 ファイル = 32 program。cartridge を仮想ディレクトリに見立てて
//!   `<cartridge>.syx/NN 名前` へ展開する（[`crate::dx7`]）。
//! - Vaporizer2: `.vvp` 1 ファイル = 1 音色。`.fxp` と同じく展開は要らない
//!   （中身は XML だが、ここでは開かない。460 ファイル 681MB を読むことになるため）。
//!
//! どれも同じ `Vec<PathBuf>` で返すので、呼び出し側（TUI の一覧・検索・カテゴリ分け）は
//! プラグインの違いを知らないまま動く。

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::dx7::{cartridge_program_component, parse_dx7_cartridge};

/// patches_dir 以下の音色をすべて列挙して返す。
/// 戻り値は絶対パス（Dexed は cartridge の下に program コンポーネントが付いた仮想パス）。
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
            continue;
        }
        match extension_lowercase(&path).as_deref() {
            Some("fxp") | Some("vvp") => list.push(path),
            Some("syx") => push_cartridge_programs(&path, list),
            _ => {}
        }
    }
    Ok(())
}

fn extension_lowercase(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// cartridge 1 個を 32 件へ展開する。
///
/// 読めない `.syx` があっても一覧全体は返す。1 ファイルの破損で音色選択が
/// まるごと使えなくなるほうが困るため。理由は stderr に 1 行出す。
fn push_cartridge_programs(cartridge: &Path, list: &mut Vec<PathBuf>) {
    let bytes = match std::fs::read(cartridge) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("cartridge を読めない {}: {}", cartridge.display(), error);
            return;
        }
    };
    let parsed = match parse_dx7_cartridge(bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!(
                "cartridge として読めない {}: {:#}",
                cartridge.display(),
                error
            );
            return;
        }
    };
    for (index, name) in parsed.program_names().iter().enumerate() {
        list.push(cartridge.join(cartridge_program_component(index, name)));
    }
}

/// パッチの絶対パスを「カテゴリ/ファイル名.fxp」形式に変換する。
/// patches_dir が `C:\ProgramData\Surge XT\patches_factory` のとき、
/// `Pads/Pad 1.fxp` のような形式になる。
/// Dexed では `SynprezFM/SynprezFM_01.syx/01 Say Again.` になる。
pub fn to_relative(patches_dir: &str, abs_path: &Path) -> String {
    let base = Path::new(patches_dir);
    abs_path
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs_path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
