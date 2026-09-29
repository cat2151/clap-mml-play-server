//! パッチリスト取得
//!
//! patches_dir 以下を再帰的に walk して、プラグインが読める音色を列挙する。
//!
//! - Surge XT: `.fxp` 1 ファイル = 1 音色。ファイルパスがそのまま音色。
//! - Dexed: `.syx` 1 ファイル = 32 program。cartridge を仮想ディレクトリに見立てて
//!   `<cartridge>.syx/NN 名前` へ展開する（[`crate::dx7`]）。
//! - Vaporizer2: `.vvp` 1 ファイル = 1 音色。`.fxp` と同じく展開は要らない
//!   （中身は XML だが、ここでは開かない。460 ファイル 681MB を読むことになるため）。
//! - Floe: `.floe-preset` 1 ファイル = 1 音色。`.floe-pkg` などは列挙しない。
//! - sforzando: `.sfz` 1 ファイル = 1 音色。
//! - Six Sines: `.sxsnp` 1 ファイル = 1 音色。
//! - TyrellN6: `.h2p` 1 ファイル = 1 音色。
//!
//! どれも同じ `Vec<PathBuf>` で返すので、呼び出し側（TUI の一覧・検索・カテゴリ分け）は
//! プラグインの違いを知らないまま動く。
//!
//! 音色選択の一覧向けの [`collect_patch_listing`] は、同じ音を鳴らす Dexed の program を
//! path 順で先頭の 1 件へまとめる。まとめられた program も path を指定すれば従来どおり読める。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::Entry, HashMap};
use std::path::{Path, PathBuf};

use crate::{
    dx7::{cartridge_program_component, parse_dx7_cartridge, voice_params_without_name},
    logging::emit_diagnostic,
};

/// 同じ音を鳴らす patch を 1 件へまとめたときの、まとめた側の情報。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MergedPatches {
    /// まとめた件数（残した 1 件を含む）。
    pub count: usize,
    /// まとめた patch の名前。重複を除き、残した 1 件の名前も含む。
    /// 残した 1 件の名前と違う別名でも一覧から検索できるようにするため。
    pub names: Vec<String>,
}

/// [`collect_patch_listing`] の 1 件。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectedPatch {
    pub path: PathBuf,
    /// 2 件以上をまとめたときだけ `Some`。
    pub merged: Option<MergedPatches>,
}

/// 走査で見つけた 1 件。cartridge の program なら、同じ音かを比べる鍵と名前を持つ。
struct FoundPatch {
    path: PathBuf,
    voice: Option<(Vec<u8>, String)>,
}

/// patches_dir 以下の音色をすべて列挙して返す。
/// 戻り値は絶対パス（Dexed は cartridge の下に program コンポーネントが付いた仮想パス）。
pub fn collect_patches(patches_dir: &str) -> Result<Vec<PathBuf>> {
    Ok(collect_sorted(patches_dir)?
        .into_iter()
        .map(|found| found.path)
        .collect())
}

/// [`collect_patches`] と同じ走査で、同じ音を鳴らす program を 1 件へまとめた一覧を返す。
pub fn collect_patch_listing(patches_dir: &str) -> Result<Vec<CollectedPatch>> {
    Ok(merge_same_voices(collect_sorted(patches_dir)?))
}

fn collect_sorted(patches_dir: &str) -> Result<Vec<FoundPatch>> {
    let mut list = Vec::new();
    visit_dir(Path::new(patches_dir), &mut list)?;
    list.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(list)
}

fn merge_same_voices(found: Vec<FoundPatch>) -> Vec<CollectedPatch> {
    let mut listed: Vec<CollectedPatch> = Vec::new();
    let mut first_by_voice: HashMap<Vec<u8>, usize> = HashMap::new();
    for patch in found {
        let Some((params, name)) = patch.voice else {
            listed.push(CollectedPatch {
                path: patch.path,
                merged: None,
            });
            continue;
        };
        match first_by_voice.entry(params) {
            Entry::Occupied(first) => {
                let merged = listed[*first.get()]
                    .merged
                    .as_mut()
                    .expect("cartridge program always starts with merged info");
                merged.count += 1;
                if !merged.names.contains(&name) {
                    merged.names.push(name);
                }
            }
            Entry::Vacant(slot) => {
                slot.insert(listed.len());
                listed.push(CollectedPatch {
                    path: patch.path,
                    merged: Some(MergedPatches {
                        count: 1,
                        names: vec![name],
                    }),
                });
            }
        }
    }
    for patch in &mut listed {
        if patch.merged.as_ref().is_some_and(|merged| merged.count < 2) {
            patch.merged = None;
        }
    }
    listed
}

fn visit_dir(dir: &Path, list: &mut Vec<FoundPatch>) -> Result<()> {
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
            Some("fxp" | "vvp" | "floe-preset" | "sfz" | "sxsnp" | "h2p") => {
                list.push(FoundPatch { path, voice: None })
            }
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
/// まるごと使えなくなるほうが困るため。理由は host process の診断 sink に 1 行出す。
fn push_cartridge_programs(cartridge: &Path, list: &mut Vec<FoundPatch>) {
    let bytes = match std::fs::read(cartridge) {
        Ok(bytes) => bytes,
        Err(error) => {
            emit_diagnostic(format!(
                "cartridge を読めない {}: {}",
                cartridge.display(),
                error
            ));
            return;
        }
    };
    let parsed = match parse_dx7_cartridge(bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            emit_diagnostic(format!(
                "cartridge として読めない {}: {:#}",
                cartridge.display(),
                error
            ));
            return;
        }
    };
    for (index, name) in parsed.program_names().iter().enumerate() {
        let params = voice_params_without_name(&parsed, index as u8);
        list.push(FoundPatch {
            path: cartridge.join(cartridge_program_component(index, name)),
            voice: Some((params, name.clone())),
        });
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
