//! DAW の cell キャッシュ WAV と、そこから作る CLAP state。
//!
//! 他の形は**音色**を指すが、これだけは**録音済みの音そのもの**を指す。それでも patch 文字列の
//! 形として扱うのは、live 経路が「patch 文字列 → プラグイン」の 1 本道でしか物理インスタンスを
//! 選べないため（`docs/adr/0007-patch-string-decides-the-plugin.md`）。state に入れるのは
//! **パスであって中身ではない**。WAV は 1 ファイル 1.6MB あり 1 小節ごとに差し替わるので、
//! 中身を流すと切り替えのたびにその量が CLAP state を通る。
//!
//! 綴りは `slot=1;C:\...\track2_meas2.wav`（スロット 1 へ載せる）/ `slot=1;`（空にする）/
//! プレフィクス無し（スロット 0）。鳴らす本は note number（`slots::slot_for_note`）が決めるので
//! 載せる本を patch 文字列で言う。組み立ては [`cache_wav_patch_with_slot`]、読みは
//! `cmrt_cache_player::slots::parse_state` で、**この 2 か所以外に綴りを書かないこと。**
//! 却下案: サフィックス形（`...\meas3.wav#1`）。拡張子が `"wav#1"` になり [`is_cache_wav_patch_path`]
//! の routing が壊れる。プレフィクス形はスロット番号が patch 文字列に収まるので SHM の版も上げない。

use std::path::Path;

use anyhow::Result;
use cmrt_cache_player::slots::{parse_state, split_slot_prefix, StateRequest};

pub use cmrt_cache_player::SLOT_COUNT;

/// DAW の cell キャッシュ WAV を指す patch 文字列か。拡張子だけで見る（他のどの形も `.wav` を使わない）。
/// スロット番号が壊れた綴り（`slot=9;` / `slot=x;`）もここでは受け取る。受け取らないと Surge XT の
/// state file へ流れ、「state のロードに失敗」という原因の遠いエラーになる。綴りの正否は [`cache_wav_state`]。
pub fn is_cache_wav_patch_path(patch: &str) -> bool {
    let (has_slot, path) = split_slot_prefix(patch);
    if has_slot && path.is_empty() {
        // `slot=1;`（空にする）と `slot=1`（区切り忘れ）はどちらも cache-player 宛てなので拡張子が無くても拾う。
        return true;
    }
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
}

/// キャッシュ WAV を指す patch 文字列を CLAP state のバイト列へ直す。綴りとファイルの存在をここで
/// 確かめ、失敗の理由をパス付きで返す（プラグインまで運ぶと「state のロードに失敗」としか分からない）。
/// 綴りは書き換えない（`slot=0;` へ正規化すると 2 か所で綴りを組み立てることになる）。
pub fn cache_wav_state(patch_path: &str) -> Result<Vec<u8>> {
    let state = patch_path.trim();
    match parse_state(state).map_err(|error| anyhow::anyhow!("{error}"))? {
        StateRequest::Load { path, .. } => {
            if !Path::new(&path).is_file() {
                anyhow::bail!("キャッシュ WAV が無い: {path}");
            }
        }
        // スロットを空にする指示。読むファイルが無いので存在検査もしない。
        StateRequest::Clear { .. } | StateRequest::ClearAll => {}
    }
    Ok(state.as_bytes().to_vec())
}

/// スロットを指定した patch 文字列を組み立てる。DAW は小節 `N` の WAV をスロット `N % SLOT_COUNT` へ
/// 載せ、その小節の note on に `60 + (N % SLOT_COUNT)` を送る。
pub fn cache_wav_patch_with_slot(slot: usize, wav_path: &str) -> String {
    cmrt_cache_player::slot_patch_state(slot, wav_path)
}

#[cfg(test)]
mod tests;
