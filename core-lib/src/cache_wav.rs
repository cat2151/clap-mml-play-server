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

use std::path::Path;

use anyhow::Result;

/// DAW の cell キャッシュ WAV を指す patch 文字列か。
///
/// 拡張子だけで見る。**他のどの形も `.wav` を使わない**ので、`.vvp` や `.sfz` と同じく
/// patch 文字列を変えずに routing できる。
pub fn is_cache_wav_patch_path(patch: &str) -> bool {
    Path::new(patch.trim())
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
}

/// キャッシュ WAV のパスを CLAP state のバイト列へ直す。
///
/// 読めないパスをプラグインまで運んでも「state のロードに失敗」としか分からないので、
/// ここで存在だけ確かめて、失敗の理由をパス付きで返す。
pub fn cache_wav_state(patch_path: &str) -> Result<Vec<u8>> {
    let path = patch_path.trim();
    if !Path::new(path).is_file() {
        anyhow::bail!("キャッシュ WAV が無い: {path}");
    }
    Ok(path.as_bytes().to_vec())
}

#[cfg(test)]
mod tests;
