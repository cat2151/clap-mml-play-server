//! 使用中プラグインの同定。
//!
//! 「いま何のプラグインを使っているか」で振る舞いを変える場所（TUI 側の voicing 判定データ源や
//! キャッシュの置き場）の材料。サーバー自身の Surge 判定は `cmrt_core::plugin_is_surge`
//! （ファイル名に surge を含むかという粗い最後の手段付き）が担当し、用途が違うので寄せていない。

use std::path::PathBuf;

pub const SURGE_XT_PLUGIN_ID: &str = "org.surge-synth-team.surge-xt";
pub const DEXED_PLUGIN_ID: &str = "com.digital-suburban.dexed";
pub const VAPORIZER2_PLUGIN_ID: &str = "com.vastdynamics.VAST2";
pub const FLOE_PLUGIN_ID: &str = "com.floe-audio.floe";
pub const SFORZANDO_PLUGIN_ID: &str = "com.Plogue Art et Technologie, Inc.sforzando";
/// DAW の cell キャッシュ WAV を鳴らす組み込みプラグイン（`cache-player` crate）の CLAP ID。
///
/// **`.clap` ファイルとしてディスクに存在しない**（バイナリへ静的リンク）ので、`installed_plugin_profiles()`
/// の「実ファイルがあるか」フィルタには載らない。実体は `cmrt_cache_player::CACHE_PLAYER_PLUGIN_ID` の写し。
pub const CACHE_PLAYER_PLUGIN_ID: &str = "org.cat2151.cmrt.cache-player";

/// `plugin_path` のファイル名から拡張子を落としたもの（`Surge XT.clap` → `Surge XT`）。
///
/// runtime の低レベル構成では `plugin_id` が無い場合もあるため、ファイル名での同定に使う。
pub fn plugin_file_stem(plugin_path: &str) -> String {
    PathBuf::from(plugin_path.trim())
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests;
