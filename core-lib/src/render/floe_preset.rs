//! Floe の `.floe-preset` を Floe 固有 CLAP extension でロードし、終わるまで待つ。

use std::time::{Duration, Instant};

use anyhow::Result;
use clack_host::prelude::PluginInstance;

use super::RealtimeRenderer;
use crate::floe::{preset_load_pending, request_preset_load, FLOE_PLUGIN_ID};
use crate::host::MidiRenderHost;

const PRESET_LOAD_TIMEOUT: Duration = Duration::from_secs(30);

impl RealtimeRenderer {
    pub(super) fn load_floe_preset(&mut self, patch_path: &str) -> Result<()> {
        ensure_floe_capable(&self.plugin_id)?;
        load_floe_state(self.plugin_instance_mut(), patch_path)
    }
}

pub(super) fn load_floe_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    patch_path: &str,
) -> Result<()> {
    let plugin = plugin_instance.plugin_handle().as_raw_ptr();
    // SAFETY: instance は main thread 上で生存中。
    unsafe { request_preset_load(plugin, std::path::Path::new(patch_path)) }?;
    wait_for_state_change(plugin_instance, patch_path)
}

fn wait_for_state_change(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    patch_path: &str,
) -> Result<()> {
    let deadline = Instant::now() + PRESET_LOAD_TIMEOUT;

    loop {
        let callback_requested =
            plugin_instance.access_shared_handler(|host| host.take_callback_request());
        if callback_requested {
            plugin_instance.call_on_main_thread_callback();
        }

        let plugin = plugin_instance.plugin_handle().as_raw_ptr();
        // SAFETY: plugin instance はこの関数の呼び出し中生存する。
        if !unsafe { preset_load_pending(plugin) }? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!("Floe preset の非同期ロードがタイムアウト '{patch_path}'");
        }
        std::thread::yield_now();
    }
}

pub(super) fn ensure_floe_capable(plugin_id: &str) -> Result<()> {
    if plugin_id == FLOE_PLUGIN_ID {
        return Ok(());
    }
    anyhow::bail!(
        "'.floe-preset' は plugin_id = '{FLOE_PLUGIN_ID}' でしか読めない（いま載っているのは '{plugin_id}'）"
    )
}
