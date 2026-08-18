//! cartridge patch の path 表現。
//!
//! cartridge を仮想ディレクトリに見立てた相対パス文字列で 1 program を指す。
//!
//! ```text
//! Surge : "Pads/Pad 1.fxp"
//! Dexed : "SynprezFM/SynprezFM_01.syx/01 Say Again."
//!          └ サブディレクトリ ┘└ cartridge ┘└ program ┘
//!                                            └ 0-based index を 2 桁 ┘
//! ```
//!
//! この形にしてあるのは、patch を「不透明な表示パス文字列」としてしか扱っていない
//! TUI 側（一覧・検索・カテゴリ分け・履歴・MML 先頭 JSON）を無改修で通すため。
//! 実際に中身を解釈するのはこのモジュールと [`super::super::render`] のロード経路だけ。
//!
//! index を 0-based のまま 2 桁ゼロ埋めで書くのは、この文字列がそのまま永続 ID に
//! なるため。UI 都合で 1-32 表示にすると、表示と保存済みデータが乖離する。

use anyhow::{bail, Result};

use super::DX7_PROGRAMS_PER_CARTRIDGE;

const SYX_EXTENSION: &str = ".syx";
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];
const PROGRAM_INDEX_DIGITS: usize = 2;

/// cartridge patch path を分解した結果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CartridgePatchPath {
    /// `.syx` ファイルまでの path。入力の前半をそのまま切り出したもの（絶対でも相対でもよい）。
    pub cartridge_path: String,
    /// 0-based program index。`0..DX7_PROGRAMS_PER_CARTRIDGE`。
    pub program_index: u8,
}

/// patch path が cartridge を指しているか。
///
/// `.syx` を名前に含む Surge の `.fxp` があれば誤判定するが、実質ありえないので
/// 型ではなく文字列で見分ける（`HANDOFF-dexed-clap-phase1.md` 1.1）。
///
/// program 部分が壊れている path も `true` にする。ここで `false` を返すと
/// `.fxp` の state load へ落ちて「ファイルを読めない」という無関係なエラーになり、
/// 書き間違いの箇所が分からなくなるため。
pub fn is_cartridge_patch_path(patch: &str) -> bool {
    patch.split(PATH_SEPARATORS).any(has_syx_extension)
}

/// cartridge patch path を分解する。[`is_cartridge_patch_path`] が `true` のものだけ渡すこと。
pub fn parse_cartridge_patch_path(patch: &str) -> Result<CartridgePatchPath> {
    let Some(separator) = patch.rfind(PATH_SEPARATORS) else {
        bail!("cartridge patch に program 番号が付いていない: '{patch}'（'<cartridge>.syx/NN 名前' の形で指定すること）");
    };
    let cartridge_path = &patch[..separator];
    let program_component = &patch[separator + 1..];
    if !last_component(cartridge_path).is_some_and(has_syx_extension) {
        bail!("cartridge patch の '.syx' が末尾から 2 番目のコンポーネントにない: '{patch}'");
    }
    let program_index = parse_program_index(program_component)
        .ok_or_else(|| anyhow::anyhow!(
            "cartridge patch の program 番号を読めない: '{program_component}'（0 から {} までの 2 桁で始めること）",
            DX7_PROGRAMS_PER_CARTRIDGE - 1
        ))?;
    Ok(CartridgePatchPath {
        cartridge_path: cartridge_path.to_string(),
        program_index,
    })
}

/// 一覧に載せる program 1 件ぶんのコンポーネント名（`"01 Say Again."`）。
pub fn cartridge_program_component(program_index: usize, program_name: &str) -> String {
    format!("{program_index:02} {program_name}")
}

fn last_component(path: &str) -> Option<&str> {
    path.rsplit(PATH_SEPARATORS).next()
}

fn has_syx_extension(component: &str) -> bool {
    component.len() > SYX_EXTENSION.len()
        && component[component.len() - SYX_EXTENSION.len()..].eq_ignore_ascii_case(SYX_EXTENSION)
}

/// 先頭 2 桁だけを見る。名前部分は読み飛ばす。
///
/// 名前を照合しないのは、sanitize の規則を後から変えても、保存済みの MML や履歴が
/// 指す program が変わらないようにするため。
fn parse_program_index(component: &str) -> Option<u8> {
    let digits = component.get(..PROGRAM_INDEX_DIGITS)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    match component.as_bytes().get(PROGRAM_INDEX_DIGITS) {
        None | Some(b' ') => {}
        Some(_) => return None,
    }
    let index: u8 = digits.parse().ok()?;
    (usize::from(index) < DX7_PROGRAMS_PER_CARTRIDGE).then_some(index)
}

#[cfg(test)]
mod tests;
