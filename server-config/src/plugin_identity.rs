//! 使用中プラグインの同定。
//!
//! 「いま何のプラグインを使っているか」で振る舞いを変えたい場所（TUI 側の voicing 判定
//! データ源やキャッシュの置き場）があるので、その材料をここに置く。
//!
//! サーバー自身の Surge 判定は `cmrt_core::plugin_is_surge`（ファイル名に surge を含むか、
//! という粗い最後の手段付き）が担当する。用途が違うので寄せていない。

use std::path::PathBuf;

pub const SURGE_XT_PLUGIN_ID: &str = "org.surge-synth-team.surge-xt";
pub const DEXED_PLUGIN_ID: &str = "com.digital-suburban.dexed";

/// `plugin_path` のファイル名から拡張子を落としたもの（`Surge XT.clap` → `Surge XT`）。
///
/// `plugin_id` は `active_plugin` / `[plugins.*]` を書いた config にしか無いのに対し、
/// `plugin_path` はどの書き方でも必ず埋まる。plugin_id が無い config の同定に使う。
pub fn plugin_file_stem(plugin_path: &str) -> String {
    PathBuf::from(plugin_path.trim())
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests;
