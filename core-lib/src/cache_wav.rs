//! DAW の cell キャッシュ WAV と、そこから作る CLAP state。
//!
//! 他の形（`.fxp` / `.syx` / `.vvp` / `.floe-preset` / `.sfz`）は**音色**を指すが、
//! これだけは**録音済みの音そのもの**を指す。それでも patch 文字列の形として扱うのは、
//! live 経路が「patch 文字列 → プラグイン」の 1 本道でしか物理インスタンスを選べないため
//! （`docs/adr/0007-patch-string-decides-the-plugin.md`）。
//!
//! # state に入れるのはパスであって中身ではない
//! `.fxp` などは**ファイルの中身**を CLAP state として流すが、キャッシュ WAV は
//! 1 ファイル 1.6MB あり、1 小節ごとに差し替わる。中身を流すと patch 切り替えのたびに
//! その量が CLAP state のストリームを通る。組み込み cache-player は自分でファイルを
//! 読めるので、**パスだけを UTF-8 で渡す**。
//!
//! # patch 文字列の綴り（**ここが単一ソース**）
//!
//! ```text
//! C:\...\track2_meas1.wav          スロット 0（スロット番号が無い従来の綴り）
//! slot=0;C:\...\track2_meas1.wav   スロット 0
//! slot=1;C:\...\track2_meas2.wav   スロット 1
//! slot=1;                          スロット 1 を空にする
//! ```
//!
//! cache-player は**同時に [`SLOT_COUNT`] 本**のキャッシュ WAV を持てる
//! （鳴っている小節・先読みした小節・その手前の小節ぶんの余裕）。
//! 本数を決めている理由は `cmrt_cache_player::slots::SLOT_COUNT` の doc。
//! どの本を鳴らすかは note number が決める
//! （`cmrt_cache_player::slots::slot_for_note` = `note % SLOT_COUNT`）ので、
//! **どの本へ載せるかを patch 文字列で言う**必要がある。
//!
//! 組み立ては [`cache_wav_patch_with_slot`]、読みは
//! `cmrt_cache_player::slots::parse_state`。プレフィクスの綴りそのものは
//! cache-player 側に閉じてあるので、**この doc とあちらの実装以外に綴りを書かないこと。**
//!
//! ## なぜプレフィクス形か（サフィックス形にはできない）
//! patch 文字列は [`is_cache_wav_patch_path`]、すなわち
//! `Path::new(patch).extension() == "wav"` で cache-player へ routing される。
//! `...\track2_meas3.wav#1` のように末尾へ足すと拡張子が `"wav#1"` になり、
//! **routing そのものが壊れて Surge XT の state file として扱われる。**
//! プレフィクス形なら最後の path component は `track2_meas3.wav` のままなので、
//! 拡張子判定に一切触れずに済む。
//!
//! ## SHM プロトコルの版は上げなくてよい
//! スロット番号は patch 文字列の中に収まっているので、`PreparePatch` の形も
//! SHM のレイアウトも変わらない。**両 repo 同時のプロトコル変更が要らない。**

use std::path::Path;

use anyhow::Result;
use cmrt_cache_player::slots::{parse_state, split_slot_prefix, StateRequest};

pub use cmrt_cache_player::SLOT_COUNT;

/// DAW の cell キャッシュ WAV を指す patch 文字列か。
///
/// 拡張子だけで見る。**他のどの形も `.wav` を使わない**ので、`.vvp` や `.sfz` と同じく
/// patch 文字列を変えずに routing できる。
///
/// スロット指定つきの綴りは、スロット番号が壊れていても（`slot=9;` / `slot=x;`）
/// ここでは受け取る。受け取らないと Surge XT の state file へ流れてしまい、
/// 「スロット番号を間違えた」が「state のロードに失敗」という原因の遠いエラーになる。
/// 綴りの正しさを判定するのは [`cache_wav_state`] の役目。
pub fn is_cache_wav_patch_path(patch: &str) -> bool {
    let (has_slot, path) = split_slot_prefix(patch);
    if has_slot && path.is_empty() {
        // `slot=1;`（そのスロットを空にする）と `slot=1`（区切りが無い誤り）。
        // どちらも cache-player 宛てなので、拡張子が無くてもここで拾う。
        return true;
    }
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
}

/// キャッシュ WAV を指す patch 文字列を CLAP state のバイト列へ直す。
///
/// 読めないパスをプラグインまで運んでも「state のロードに失敗」としか分からないので、
/// ここで綴りとファイルの存在を確かめて、失敗の理由をパス付きで返す。
///
/// 通すのは**綴りそのまま**（前後の空白を落としただけ）。スロット番号なしの綴りを
/// `slot=0;` へ書き換えたりはしない。cache-player 側が同じ解釈をするので、
/// 書き換えると 2 か所で綴りを組み立てることになるだけ。
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

/// スロットを指定した patch 文字列を組み立てる。
///
/// DAW は小節 index `N` の WAV をスロット `N % SLOT_COUNT` へ載せ、その小節の note on に
/// `60 + (N % SLOT_COUNT)` を送る。こうすると「小節 N を鳴らしている最中に小節 N+1 を
/// 別スロットへ載せておく」ができる。
pub fn cache_wav_patch_with_slot(slot: usize, wav_path: &str) -> String {
    cmrt_cache_player::slot_patch_state(slot, wav_path)
}

#[cfg(test)]
mod tests;
