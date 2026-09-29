//! config.toml の置き場所。
//!
//! サーバーは TUI アプリ（clap-mml-render-tui）の一部として同じ config.toml を読むので、
//! ディレクトリ名も TUI と同じものを使う。

use std::path::PathBuf;

const APP_DIR_NAME: &str = "clap-mml-render-tui";

/// OS 標準の設定ディレクトリ内のアプリ設定ディレクトリを返す。
/// - Windows: %LOCALAPPDATA%\clap-mml-render-tui  (Local 側)
/// - Linux:   ~/.config/clap-mml-render-tui
/// - macOS:   ~/Library/Application Support/clap-mml-render-tui
///
/// システムの設定ディレクトリが取得できない場合は `None` を返す。
pub fn config_app_dir() -> Option<PathBuf> {
    dirs::config_local_dir().map(|d| d.join(APP_DIR_NAME))
}

pub fn config_file_path() -> Option<PathBuf> {
    config_app_dir().map(|d| d.join("config.toml"))
}

/// GitHub から取得した Six Sines の factory 音色の置き場。置き場からの相対パスが音色の永続 ID なので、
/// パスに plugin の commit を含めない（含めると plugin 更新で保存済みの patch 文字列が解決しなくなる）。
pub fn six_sines_factory_dir() -> Option<PathBuf> {
    config_app_dir().map(|d| d.join("vendor-patches").join("six-sines-factory"))
}
