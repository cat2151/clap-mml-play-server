//! CLAP プラグインインスタンスの生成。
//!
//! `create_plugin()` + `init()` はサーバー起動時間のほぼ全部を占める重い処理で、
//! その中身は Surge XT 内部の初期化。ホスト側から軽くする手は
//! [`crate::surge_data`]（データディレクトリを絞る）と
//! [`super::parallel`]（複数インスタンスを並列に作る）の 2 つ。

use std::ffi::CString;

use anyhow::Result;
use clack_host::prelude::*;

use super::descriptor::{select_descriptor, SelectedDescriptor};
use super::patch_state::load_patch;
use crate::dx7::is_cartridge_patch_path;
use crate::host::{MidiRenderHost, MidiRenderHostShared};
use crate::CoreConfig;

/// `activate()` していない instance を作り、`.fxp` パッチだけロードして返す。
///
/// cartridge patch（Dexed）は SysEx event なので `process()` を通さないと送れない。
/// ここでは扱えないため、黙って初期音色で返さずエラーにする。cartridge を使う経路は
/// [`super::RealtimeRenderer`] を通すこと。
pub fn create_plugin_instance(
    cfg: &CoreConfig,
    entry: &PluginEntry,
) -> Result<PluginInstance<MidiRenderHost>> {
    let descriptor = select_descriptor(entry, cfg.plugin_id.as_deref())?;
    let mut plugin_instance = create_plugin_instance_without_patch(entry, &descriptor)?;

    if let Some(ref patch) = cfg.patch_path {
        if is_cartridge_patch_path(patch) {
            anyhow::bail!(
                "cartridge patch は activate 前にはロードできない '{patch}'（RealtimeRenderer を使うこと）"
            );
        }
        load_patch(&mut plugin_instance, patch)?;
    }

    Ok(plugin_instance)
}

pub(super) fn create_plugin_instance_without_patch(
    entry: &PluginEntry,
    descriptor: &SelectedDescriptor,
) -> Result<PluginInstance<MidiRenderHost>> {
    let plugin_id = CString::new(descriptor.id.as_str())
        .map_err(|_| anyhow::anyhow!("プラグインIDに NUL が含まれる: {}", descriptor.id))?;

    let host_info = HostInfo::new(
        "clap-midi-render",
        "clap-midi-render",
        "https://example.com",
        "0.1.0",
    )?;
    let plugin_instance = PluginInstance::<MidiRenderHost>::new(
        |_| MidiRenderHostShared,
        |_| (),
        entry,
        &plugin_id,
        &host_info,
    )?;

    Ok(plugin_instance)
}
