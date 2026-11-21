//! Apricot は内蔵 factory を PLUGIN location、user file を FILE location で読む。
use std::ffi::CString;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use clack_host::prelude::PluginInstance;
use clap_sys::ext::preset_load::{
    clap_plugin_preset_load, CLAP_EXT_PRESET_LOAD, CLAP_EXT_PRESET_LOAD_COMPAT,
};
use clap_sys::factory::preset_discovery::{
    CLAP_PRESET_DISCOVERY_LOCATION_FILE, CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN,
};
use cmrt_server_config::APRICOT_PLUGIN_ID;

use crate::host::MidiRenderHost;

pub(super) fn load_apricot_preset(
    instance: &mut PluginInstance<MidiRenderHost>,
    plugin_id: &str,
    patch: &str,
) -> Result<()> {
    ensure!(
        plugin_id == APRICOT_PLUGIN_ID,
        ".synthpatch は Apricot 用（現在 {plugin_id}）"
    );
    let path = Path::new(patch);
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("synthpatch")),
        "Apricot は .synthpatch を要求する"
    );
    let factory = plugin_presets::apricot::factory_patch(path);
    let location_text;
    let key;
    let (location, load_key) = if let Some((bundle, factory_key)) = factory {
        ensure!(
            bundle.is_file(),
            "Apricot bundle が無い: {}",
            bundle.display()
        );
        key = CString::new(factory_key)?;
        (
            (CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN, std::ptr::null()),
            key.as_ptr(),
        )
    } else {
        ensure!(path.is_file(), "Apricot user 音色が無い: {patch}");
        location_text = CString::new(patch)?;
        (
            (CLAP_PRESET_DISCOVERY_LOCATION_FILE, location_text.as_ptr()),
            std::ptr::null(),
        )
    };
    let handle = instance.plugin_handle();
    let get_extension = handle
        .as_raw()
        .get_extension
        .context("get_extension が無い")?;
    // SAFETY: live instance, main thread, ABI IDs from clap-sys. clack only
    // queries the first extension identifier, so explicitly try draft/2 too.
    unsafe {
        let loader = [CLAP_EXT_PRESET_LOAD, CLAP_EXT_PRESET_LOAD_COMPAT]
            .into_iter()
            .find_map(|id| {
                get_extension(handle.as_raw_ptr(), id.as_ptr())
                    .cast::<clap_plugin_preset_load>()
                    .as_ref()
            })
            .context("Apricot preset-load が無い")?;
        ensure!(
            loader.from_location.context("from_location が無い")?(
                handle.as_raw_ptr(),
                location.0,
                location.1,
                load_key
            ),
            "Apricot の音色を読めない: {patch}"
        );
    }
    Ok(())
}
