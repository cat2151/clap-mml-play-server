//! 生の `clap_plugin` を受け取り、Floe 固有 CLAP extension で `.floe-preset` をロードする。
//!
//! clack の instance を持たない host（別の CLAP host が作った instance）からも呼べる。

use std::ffi::{c_char, c_void, CStr};
use std::path::Path;

use anyhow::{Context, Result};
use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::clap_ostream;

const FLOE_EXTENSION_ID: &CStr = c"floe.floe";

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

/// `clap.state.save` の write callback で、最初の 1 回だけ Floe の preset loader を呼ぶ。
///
/// Floe の固有 loader は論理 main thread 上での呼び出しを前提にしている。通常の CLAP host から
/// 固有 loader を直接呼んでも Floe 側はその scope を作らない。`clap.state.save` は scope を
/// 作った後に ostream を呼ぶため、ここを同期 callback として使う。
struct PresetLoadWriter {
    plugin: *const clap_plugin,
    load_preset: unsafe extern "C" fn(*const c_void, FloeString) -> bool,
    path: FloeString,
    called: bool,
    loaded: bool,
}

unsafe extern "C" fn write_once(
    stream: *const clap_ostream,
    _buffer: *const c_void,
    size: u64,
) -> i64 {
    // SAFETY: ctx は request_preset_load の stack 上の PresetLoadWriter を指し、save の間生存する。
    let writer = unsafe { &mut *(*stream).ctx.cast::<PresetLoadWriter>() };
    if !writer.called {
        writer.called = true;
        // SAFETY: plugin と path は state.save 呼び出し全体で生存している。
        writer.loaded = unsafe { (writer.load_preset)(writer.plugin.cast(), writer.path) };
    }
    size as i64
}

/// `plugin` の Floe 固有 extension。
///
/// # Safety
/// `plugin` は生存中の CLAP plugin instance。
unsafe fn extension<'a>(plugin: *const clap_plugin) -> Result<&'a FloeClapExtension> {
    let get_extension = unsafe { (*plugin).get_extension }
        .ok_or_else(|| anyhow::anyhow!("Floe plugin に get_extension がない"))?;
    let extension =
        unsafe { get_extension(plugin, FLOE_EXTENSION_ID.as_ptr()) }.cast::<FloeClapExtension>();
    unsafe { extension.as_ref() }.ok_or_else(|| {
        anyhow::anyhow!(
            "Floe 固有 extension '{}' がない",
            FLOE_EXTENSION_ID.to_string_lossy()
        )
    })
}

/// Floe instance `plugin` に `patch_path` の preset のロードを要求する。
///
/// 返った時点ではロードは終わっていない。Floe は host の main thread callback
/// （`request_callback` → `on_main_thread`）の中でロードを進める。終わったかは
/// [`preset_load_pending`] で分かる。
///
/// # Safety
/// `plugin` は生存中の CLAP plugin instance で、呼び出しは CLAP の main thread から行う。
pub unsafe fn request_preset_load(plugin: *const clap_plugin, patch_path: &Path) -> Result<()> {
    std::fs::metadata(patch_path)
        .with_context(|| format!("Floe preset を読めない '{}'", patch_path.display()))?;
    let absolute = if patch_path.is_absolute() {
        patch_path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("current directory を取得できない")?
            .join(patch_path)
    };
    let absolute = absolute.to_str().ok_or_else(|| {
        anyhow::anyhow!(
            "Floe preset path を UTF-8 に変換できない: {}",
            absolute.display()
        )
    })?;
    let load_preset = unsafe { extension(plugin) }?
        .load_preset_file
        .ok_or_else(|| anyhow::anyhow!("Floe 固有 extension に load_preset_file がない"))?;
    let get_extension = unsafe { (*plugin).get_extension }
        .ok_or_else(|| anyhow::anyhow!("Floe plugin に get_extension がない"))?;
    let state =
        unsafe { get_extension(plugin, CLAP_EXT_STATE.as_ptr()) }.cast::<clap_plugin_state>();
    let save = unsafe { state.as_ref() }
        .and_then(|state| state.save)
        .ok_or_else(|| anyhow::anyhow!("Floe plugin に clap.state extension がない"))?;
    let mut writer = PresetLoadWriter {
        plugin,
        load_preset,
        path: FloeString {
            data: absolute.as_ptr().cast(),
            size: absolute.len(),
        },
        called: false,
        loaded: false,
    };
    let stream = clap_ostream {
        ctx: (&mut writer as *mut PresetLoadWriter).cast(),
        write: Some(write_once),
    };
    if !unsafe { save(plugin, &stream) } {
        anyhow::bail!("Floe preset loader を main thread で呼べない");
    }
    if !writer.called || !writer.loaded {
        anyhow::bail!("Floe preset のロード要求に失敗 '{absolute}'");
    }
    Ok(())
}

/// [`request_preset_load`] で要求したロードがまだ終わっていないか。
///
/// # Safety
/// `plugin` は生存中の CLAP plugin instance。
pub unsafe fn preset_load_pending(plugin: *const clap_plugin) -> Result<bool> {
    let pending = unsafe { extension(plugin) }?
        .state_change_is_pending
        .ok_or_else(|| anyhow::anyhow!("Floe 固有 extension に state_change_is_pending がない"))?;
    Ok(unsafe { pending(plugin.cast()) })
}
