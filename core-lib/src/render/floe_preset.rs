//! Floe の `.floe-preset` を Floe 固有 CLAP extension でロードする。

use std::ffi::{c_char, c_void, CStr};
use std::io::{self, Write};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clack_extensions::state::PluginState;
use clack_host::prelude::PluginInstance;

use super::RealtimeRenderer;
use crate::floe::FLOE_PLUGIN_ID;
use crate::host::MidiRenderHost;

const FLOE_EXTENSION_ID: &CStr = c"floe.floe";
const PRESET_LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Floe の `String` (`Span<char const>`) と同じ ABI。
#[repr(C)]
#[derive(Clone, Copy)]
struct FloeString {
    data: *const c_char,
    size: usize,
}

/// Floe 公開ソースの `FloeClapExtension` と同じ field order。
///
/// 先行 field の関数 signature は呼ばないため opaque pointer として扱う。
#[repr(C)]
struct FloeClapExtension {
    state_change_is_pending: Option<unsafe extern "C" fn(*const c_void) -> bool>,
    save_gui_state: *const c_void,
    load_gui_state: *const c_void,
    request_screenshot: *const c_void,
    screenshot_request_pending: *const c_void,
    load_preset_file: Option<unsafe extern "C" fn(*const c_void, FloeString) -> bool>,
}

/// `clap.state.save` の write callback 内で Floe の preset loader を呼ぶ。
///
/// Floe の固有 loader は論理 main thread 上での呼び出しを前提にしている。一方、通常の
/// CLAP host から固有 loader を直接呼んでも Floe 側はその scope を作らない。
/// `clap.state.save` は scope を作った後に ostream を呼ぶため、ここを同期 callback として使う。
struct PresetLoadWriter {
    plugin: *const c_void,
    load_preset: unsafe extern "C" fn(*const c_void, FloeString) -> bool,
    path: FloeString,
    called: bool,
    loaded: bool,
}

impl Write for PresetLoadWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.called {
            self.called = true;
            // SAFETY: plugin と path は state.save 呼び出し全体で生存している。
            self.loaded = unsafe { (self.load_preset)(self.plugin, self.path) };
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

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
    std::fs::metadata(patch_path)
        .with_context(|| format!("Floe preset を読めない '{patch_path}'"))?;
    let path = std::path::Path::new(patch_path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("current directory を取得できない")?
            .join(path)
    };
    let absolute = absolute.to_str().ok_or_else(|| {
        anyhow::anyhow!(
            "Floe preset path を UTF-8 に変換できない: {}",
            absolute.display()
        )
    })?;

    let handle = plugin_instance.plugin_handle();
    let Some(get_extension) = handle.as_raw().get_extension else {
        anyhow::bail!("Floe plugin に get_extension がない");
    };
    // SAFETY: instance は main thread 上で生存中。Floe の公開 extension ID へ問い合わせる。
    let extension = unsafe {
        get_extension(handle.as_raw_ptr(), FLOE_EXTENSION_ID.as_ptr()).cast::<FloeClapExtension>()
    };
    let extension = unsafe { extension.as_ref() }.ok_or_else(|| {
        anyhow::anyhow!(
            "Floe 固有 extension '{}' がない",
            FLOE_EXTENSION_ID.to_string_lossy()
        )
    })?;
    let load_preset = extension
        .load_preset_file
        .ok_or_else(|| anyhow::anyhow!("Floe 固有 extension に load_preset_file がない"))?;

    let path = FloeString {
        data: absolute.as_ptr().cast(),
        size: absolute.len(),
    };
    let state = handle
        .get_extension::<PluginState>()
        .ok_or_else(|| anyhow::anyhow!("Floe plugin に clap.state extension がない"))?;
    let mut writer = PresetLoadWriter {
        plugin: handle.as_raw_ptr().cast(),
        load_preset,
        path,
        called: false,
        loaded: false,
    };
    state
        .save(&handle, &mut writer)
        .map_err(|_| anyhow::anyhow!("Floe preset loader を main thread で呼べない"))?;
    if !writer.called || !writer.loaded {
        anyhow::bail!("Floe preset のロード要求に失敗 '{absolute}'");
    }
    wait_for_state_change(plugin_instance, extension, absolute)
}

fn wait_for_state_change(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    extension: &FloeClapExtension,
    patch_path: &str,
) -> Result<()> {
    let pending = extension
        .state_change_is_pending
        .ok_or_else(|| anyhow::anyhow!("Floe 固有 extension に state_change_is_pending がない"))?;
    let deadline = Instant::now() + PRESET_LOAD_TIMEOUT;

    loop {
        let callback_requested =
            plugin_instance.access_shared_handler(|host| host.take_callback_request());
        if callback_requested {
            plugin_instance.call_on_main_thread_callback();
        }

        let handle = plugin_instance.plugin_handle();
        // SAFETY: extension と plugin instance はともにこの関数の呼び出し中生存する。
        let is_pending = unsafe { pending(handle.as_raw_ptr().cast()) };
        if !is_pending {
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
