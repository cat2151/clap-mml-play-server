//! CLAP プラグインインスタンスの生成。
//!
//! `create_plugin()` + `init()` はサーバー起動時間のほぼ全部を占める重い処理で、
//! その中身は Surge XT 内部の初期化。ホスト側から軽くする手は
//! [`crate::surge_data`]（データディレクトリを絞る）と
//! [`super::parallel`]（複数インスタンスを並列に作る）の 2 つ。

use anyhow::Result;
use clack_host::prelude::*;

use super::patch_state::load_patch;
use crate::host::{MidiRenderHost, MidiRenderHostShared};
use crate::CoreConfig;

pub fn create_plugin_instance(
    cfg: &CoreConfig,
    entry: &PluginEntry,
) -> Result<PluginInstance<MidiRenderHost>> {
    let mut plugin_instance = create_plugin_instance_without_patch(entry)?;

    if let Some(ref patch) = cfg.patch_path {
        load_patch(&mut plugin_instance, patch)?;
    }

    Ok(plugin_instance)
}

pub(super) fn create_plugin_instance_without_patch(
    entry: &PluginEntry,
) -> Result<PluginInstance<MidiRenderHost>> {
    let plugin_factory = entry
        .get_plugin_factory()
        .ok_or_else(|| anyhow::anyhow!("PluginFactory が見つからない"))?;
    let plugin_descriptor = plugin_factory
        .plugin_descriptors()
        .next()
        .ok_or_else(|| anyhow::anyhow!("プラグインディスクリプタが見つからない"))?;

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
        plugin_descriptor.id().unwrap(),
        &host_info,
    )?;

    Ok(plugin_instance)
}
