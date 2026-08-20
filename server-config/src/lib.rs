//! サーバー（render-server / realtime-play-server）が読む config.toml。
//!
//! config.toml そのものは TUI アプリ（clap-mml-render-tui）と共有しているが、
//! **サーバーが読む項目だけ**をここで宣言する。TUI 固有の項目（`loop_dirs` や
//! 用途別 patch カテゴリなど）は未知キーとして無視される。
//!
//! この crate が play server repo 側にあることで、サーバーは TUI repo へ一切依存せず、
//! repo 間の依存が「TUI → play server」の一方向になる。プラグインの標準インストール先や
//! `active_plugin` の解決規則も、プラグインをロードするこちら側の知識としてここが持つ。

mod patch_dirs;
mod paths;
mod plugin_defaults;
mod plugin_identity;
mod plugin_profile;

pub use patch_dirs::{configured_patch_dirs, patch_root_dir, shared_patch_root_dir};
pub use paths::{config_app_dir, config_file_path};
pub use plugin_defaults::{
    default_dexed_cartridge_dirs, default_dexed_plugin_path, default_patches_dirs,
    default_plugin_path,
};
pub use plugin_identity::{plugin_file_stem, DEXED_PLUGIN_ID, SURGE_XT_PLUGIN_ID};
pub use plugin_profile::{
    builtin_plugin_profiles, installed_plugin_profiles, merged_plugin_profiles, patch_form_of,
    resolve_active_plugin_profile, PatchForm, PatchRoleFilters, PluginProfile,
};

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use serde::Deserialize;

pub const DEFAULT_OFFLINE_RENDER_SERVER_WORKERS: usize = 4;
pub const DEFAULT_OFFLINE_RENDER_SERVER_PORT: u16 = 62153;
pub const DEFAULT_REALTIME_PLAY_SERVER_PORT: u16 = 62154;
const MIN_OFFLINE_RENDER_WORKERS: usize = 1;
const MAX_OFFLINE_RENDER_WORKERS: usize = 16;

/// config.toml のうち、サーバーが読む項目。
///
/// TUI 側の `cmrt_runtime::Config` とは別宣言だが、同じ TOML キーを指している。
/// 「サーバーが何を読むか」がこの構造体だけで分かるようにするのが狙い。
/// 項目を増やすときは TUI 側のキー名と必ず合わせること。
#[derive(Deserialize, Debug, Clone)]
pub struct ServerConfig {
    /// 使用するプラグインのパス。`active_plugin` を使う config では書かないので、
    /// 省略を許して空文字にする（空のまま使われた場合は読み手が「空です」と弾く）。
    #[serde(default)]
    pub plugin_path: String,
    /// 使用中プラグインの CLAP plugin ID。プロファイル解決後の値が入る。
    #[serde(default)]
    pub plugin_id: Option<String>,
    /// 使う `[plugins.*]` の名前。未指定ならトップレベルの指定をそのまま使う（後方互換）。
    #[serde(default)]
    pub active_plugin: Option<String>,
    /// プラグインごとの設定。`active_plugin` が指すものだけが使われる。
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginProfile>,
    pub output_midi: String,
    pub output_wav: String,
    pub sample_rate: f64,
    pub buffer_size: usize,
    /// パッチ検索対象ディレクトリ一覧
    pub patches_dirs: Option<Vec<String>>,
    /// render-server backend のオフラインレンダリング同時実行数
    #[serde(default = "default_offline_render_server_workers")]
    pub offline_render_server_workers: usize,
    /// render-server backend が使う localhost port
    #[serde(default = "default_offline_render_server_port")]
    pub offline_render_server_port: u16,
    /// realtime play server backend が使う localhost port
    #[serde(default = "default_realtime_play_server_port")]
    pub realtime_play_server_port: u16,
}

impl ServerConfig {
    /// config.toml を読んで `active_plugin` を解決する。
    ///
    /// TUI と違い、無い場合にひな形を書き出すことはしない。ひな形は TUI 固有の項目まで
    /// 含むので、TUI 側 (`cmrt_runtime::Config::load`) だけが持つ責務にしてある。
    pub fn load() -> Result<Self> {
        let path = config_file_path().ok_or_else(|| {
            anyhow::anyhow!(
                "システムの設定ディレクトリが取得できません。HOME 環境変数などを確認してください。"
            )
        })?;
        let text = std::fs::read_to_string(&path).with_context(|| {
            format!(
                "config.toml が読めない ({})。先に clap-mml-render-tui を一度起動して\
                 config.toml を作成してください。",
                path.display()
            )
        })?;
        Self::from_toml_str(&text)
            .with_context(|| format!("config.toml の読み込みに失敗 ({})", path.display()))
    }

    /// `load` の中身のうち、ファイルに触らない部分。
    pub fn from_toml_str(text: &str) -> Result<Self> {
        let mut cfg: Self = toml::from_str(text).context("config.toml のパースに失敗")?;
        cfg.apply_active_plugin_profile()
            .context("config.toml のプラグイン設定が不正")?;
        cfg.validate().context("config.toml の検証に失敗")?;
        Ok(cfg)
    }

    /// `active_plugin` が指すプロファイルの値をトップレベルフィールドへ焼き込む。
    fn apply_active_plugin_profile(&mut self) -> Result<()> {
        let resolved = resolve_active_plugin_profile(
            self.active_plugin.as_deref(),
            &self.plugins,
            &self.plugin_path,
        )?;
        if let Some(profile) = resolved {
            self.plugin_path = profile.plugin_path;
            self.plugin_id = profile.plugin_id;
            self.patches_dirs = profile.patches_dirs;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        validate_offline_render_workers(
            "offline_render_server_workers",
            self.offline_render_server_workers,
        )?;
        if self.offline_render_server_port == 0 {
            anyhow::bail!("offline_render_server_port は 1〜65535 の範囲で設定してください");
        }
        if self.realtime_play_server_port == 0 {
            anyhow::bail!("realtime_play_server_port は 1〜65535 の範囲で設定してください");
        }
        Ok(())
    }

    /// `CoreConfig.patches_dir` へ渡す 1 本のディレクトリ。
    pub fn patch_root_dir(&self) -> Option<String> {
        patch_root_dir(self.patches_dirs.as_deref())
    }

    /// このマシンで実際に使えるプラグインのプロファイル（`plugin_path` が実在するものだけ）。
    ///
    /// `active_plugin` が指す 1 つではなく、**同じプロセスに同時に載せられる候補**を返す。
    /// 再生サーバーの予備インスタンスプールが、行ごとに違うプラグインを載せるために使う。
    pub fn installed_plugin_profiles(&self) -> BTreeMap<String, PluginProfile> {
        installed_plugin_profiles(&self.plugins)
    }
}

fn default_offline_render_server_workers() -> usize {
    DEFAULT_OFFLINE_RENDER_SERVER_WORKERS
}

fn default_offline_render_server_port() -> u16 {
    DEFAULT_OFFLINE_RENDER_SERVER_PORT
}

fn default_realtime_play_server_port() -> u16 {
    DEFAULT_REALTIME_PLAY_SERVER_PORT
}

fn validate_offline_render_workers(name: &str, workers: usize) -> Result<()> {
    if !(MIN_OFFLINE_RENDER_WORKERS..=MAX_OFFLINE_RENDER_WORKERS).contains(&workers) {
        anyhow::bail!(
            "{} は {}〜{} の範囲で設定してください（現在値: {}）",
            name,
            MIN_OFFLINE_RENDER_WORKERS,
            MAX_OFFLINE_RENDER_WORKERS,
            workers
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
