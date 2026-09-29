//! plugin 本体に埋め込まれていてディスクに無い音色を、取得してディスクへ置く入口。
//!
//! どの plugin に取得が要るかはここで決める。呼び出し側（catalog 構築）は plugin 名で
//! 分岐せず、結果の行を表示するだけでよい。

use std::collections::BTreeMap;
use std::fmt;

use anyhow::Result;
use cmrt_server_config::{patch_form_of, PatchForm, PluginProfile};

use crate::six_sines_factory::{sync_six_sines_factory, FactorySync};

/// 1 plugin ぶんの取得結果。`Display` は「`<plugin 名>: <結果>`」の 1 行。
#[derive(Debug)]
pub struct PatchDownload {
    pub plugin_name: String,
    pub result: Result<FactorySync>,
}

impl fmt::Display for PatchDownload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.result {
            Ok(sync) => write!(f, "{}: 音色の取得: {sync}", self.plugin_name),
            Err(error) => write!(f, "{}: 音色の取得に失敗: {error:#}", self.plugin_name),
        }
    }
}

/// インストール済みの plugin のうち、音色の取得が要るものについて取得を行う。
///
/// 1 つが失敗しても残りは続ける。取得が要る plugin が無ければ空を返す。
pub fn prepare_downloaded_patches(
    from_config: &BTreeMap<String, PluginProfile>,
) -> Vec<PatchDownload> {
    prepare_downloaded_patches_with(
        cmrt_server_config::installed_plugin_profiles(from_config),
        sync_six_sines_factory,
    )
}

/// [`prepare_downloaded_patches`] の、対象プロファイルと Six Sines の取得を差し替えられる版。
pub fn prepare_downloaded_patches_with(
    installed: BTreeMap<String, PluginProfile>,
    mut sync_six_sines: impl FnMut(&str) -> Result<FactorySync>,
) -> Vec<PatchDownload> {
    installed
        .into_iter()
        .filter(|(_, profile)| {
            patch_form_of(profile.plugin_id.as_deref(), &profile.plugin_path) == PatchForm::SixSines
        })
        .map(|(plugin_name, profile)| PatchDownload {
            plugin_name,
            result: sync_six_sines(&profile.plugin_path),
        })
        .collect()
}

#[cfg(test)]
mod tests;
