//! CLAP プラグインインスタンスの生成。
//!
//! `create_plugin()` + `init()` はサーバー起動時間のほぼ全部を占める重い処理で、
//! その中身は Surge XT 内部の初期化。ホスト側から軽くする手は
//! [`crate::surge_data`]（データディレクトリを絞る）と
//! [`super::parallel`]（複数インスタンスを並列に作る）の 2 つ。
//!
//! ただし**並列に作れないプラグインがある**（Vaporizer2 は 2 スレッドで segfault する）。
//! ここは instance を作る唯一の入口なので、直列化もここで掛ける
//! （[`super::serial_instantiation`]）。生成の呼び出し側が増えても掛け忘れない。

use std::ffi::CString;

use anyhow::Result;
use clack_host::prelude::*;

use super::descriptor::SelectedDescriptor;
use super::serial_instantiation::InstantiationPermit;
use crate::host::{MidiRenderHost, MidiRenderHostShared};

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
    // 並列に作れないプラグインを他の生成から隔離する。落ちるのは create_plugin() + init()
    // なので、保持するのはこの区間だけでよい（doc は serial_instantiation を見ること）。
    let _permit = InstantiationPermit::acquire(&descriptor.id);
    let plugin_instance = PluginInstance::<MidiRenderHost>::new(
        |_| MidiRenderHostShared,
        |_| (),
        entry,
        &plugin_id,
        &host_info,
    )?;

    Ok(plugin_instance)
}
