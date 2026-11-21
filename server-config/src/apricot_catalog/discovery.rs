//! clack の get_factory は先頭 ID だけを問い合わせるため、Apricot の draft-2 を直接呼ぶ。
use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use clack_host::prelude::PluginEntry;
use clap_sys::factory::preset_discovery::*;
use clap_sys::universal_plugin_id::clap_universal_plugin_id;
use clap_sys::version::CLAP_VERSION;

#[derive(Default)]
struct Discovery {
    plugin_location: bool,
    keys: Vec<String>,
    error: Option<String>,
}

// All callbacks run synchronously during provider.init/get_metadata. The boxed
// Discovery and stack ABI objects outlive the provider, which is destroyed first.
unsafe extern "C" fn filetype(
    _: *const clap_preset_discovery_indexer,
    _: *const clap_preset_discovery_filetype,
) -> bool {
    true
}
unsafe extern "C" fn soundpack(
    _: *const clap_preset_discovery_indexer,
    _: *const clap_preset_discovery_soundpack,
) -> bool {
    true
}
unsafe extern "C" fn extension(
    _: *const clap_preset_discovery_indexer,
    _: *const c_char,
) -> *const c_void {
    std::ptr::null()
}
unsafe extern "C" fn location(
    indexer: *const clap_preset_discovery_indexer,
    location: *const clap_preset_discovery_location,
) -> bool {
    // SAFETY: CLAP passes the indexer and valid location during init only.
    unsafe {
        let data = &mut *((*indexer).indexer_data.cast::<Discovery>());
        data.plugin_location |= (*location).kind == CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN;
    }
    true
}
unsafe extern "C" fn begin(
    receiver: *const clap_preset_discovery_metadata_receiver,
    _: *const c_char,
    key: *const c_char,
) -> bool {
    // SAFETY: receiver_data points at our live Discovery; key is a nullable CLAP C string.
    unsafe {
        let data = &mut *((*receiver).receiver_data.cast::<Discovery>());
        if let Some(key) = key.as_ref().and_then(|_| CStr::from_ptr(key).to_str().ok()) {
            if plugin_presets::apricot::valid_factory_key(key) {
                data.keys.push(key.to_owned());
                return true;
            }
        }
        data.error = Some("Apricot factory key が不正".into());
    }
    false
}
unsafe extern "C" fn error(
    receiver: *const clap_preset_discovery_metadata_receiver,
    code: i32,
    _: *const c_char,
) {
    // SAFETY: receiver_data points at our live Discovery.
    unsafe {
        (*((*receiver).receiver_data.cast::<Discovery>())).error =
            Some(format!("Apricot discovery error {code}"));
    }
}
unsafe extern "C" fn plugin_id(
    _: *const clap_preset_discovery_metadata_receiver,
    _: *const clap_universal_plugin_id,
) {
}
unsafe extern "C" fn text(_: *const clap_preset_discovery_metadata_receiver, _: *const c_char) {}
unsafe extern "C" fn flags(_: *const clap_preset_discovery_metadata_receiver, _: u32) {}
unsafe extern "C" fn timestamps(_: *const clap_preset_discovery_metadata_receiver, _: u64, _: u64) {
}
unsafe extern "C" fn extra(
    _: *const clap_preset_discovery_metadata_receiver,
    _: *const c_char,
    _: *const c_char,
) {
}

struct Provider(*const clap_preset_discovery_provider);
impl Drop for Provider {
    fn drop(&mut self) {
        // SAFETY: create succeeded, entry and callback storage still live. Destroy exactly once.
        unsafe {
            if let Some(destroy) = (*self.0).destroy {
                destroy(self.0);
            }
        }
    }
}

/// No GUI, plugin instance, audio device, or persistent file is created.
pub(super) fn factory_patches(bundle: &Path) -> Result<Vec<PathBuf>> {
    // SAFETY: the caller selected this installed CLAP bundle.
    let entry = unsafe { PluginEntry::load(bundle) }.context("Apricot のロードに失敗")?;
    let get_factory = entry
        .raw_entry()
        .get_factory
        .context("get_factory が無い")?;
    let mut data = Box::<Discovery>::default();
    let data_ptr = (&mut *data as *mut Discovery).cast::<c_void>();
    let indexer = clap_preset_discovery_indexer {
        clap_version: CLAP_VERSION,
        name: c"cmrt-catalog".as_ptr(),
        vendor: c"cmrt".as_ptr(),
        url: c"".as_ptr(),
        version: c"0.1.0".as_ptr(),
        indexer_data: data_ptr,
        declare_filetype: Some(filetype),
        declare_location: Some(location),
        declare_soundpack: Some(soundpack),
        get_extension: Some(extension),
    };
    let receiver = clap_preset_discovery_metadata_receiver {
        receiver_data: data_ptr,
        on_error: Some(error),
        begin_preset: Some(begin),
        add_plugin_id: Some(plugin_id),
        set_soundpack_id: Some(text),
        set_flags: Some(flags),
        add_creator: Some(text),
        set_description: Some(text),
        set_timestamps: Some(timestamps),
        add_feature: Some(text),
        add_extra_info: Some(extra),
    };
    // SAFETY: ABI and compatibility IDs come from clap-sys. Returned pointers
    // remain valid while entry/provider live; all calls stay on this thread.
    unsafe {
        let factory = [
            CLAP_PRESET_DISCOVERY_FACTORY_ID,
            CLAP_PRESET_DISCOVERY_FACTORY_ID_COMPAT,
        ]
        .into_iter()
        .find_map(|id| {
            get_factory(id.as_ptr())
                .cast::<clap_preset_discovery_factory>()
                .as_ref()
        })
        .context("Apricot preset discovery が無い")?;
        let count = factory.count.context("provider count が無い")?(factory);
        let descriptor = factory
            .get_descriptor
            .context("provider descriptor が無い")?;
        let found = (0..count).any(|i| {
            descriptor(factory, i).as_ref().is_some_and(|d| {
                !d.id.is_null()
                    && CStr::from_ptr(d.id) == c"nakst.Apricot.preset_discovery_provider"
            })
        });
        ensure!(found, "Apricot provider が無い");
        let raw = factory.create.context("provider create が無い")?(
            factory,
            &indexer,
            c"nakst.Apricot.preset_discovery_provider".as_ptr(),
        );
        let provider_ref = raw.as_ref().context("Apricot provider を生成できない")?;
        let provider = Provider(raw);
        ensure!(
            provider_ref.init.context("provider init が無い")?(raw),
            "Apricot provider init に失敗"
        );
        ensure!(data.plugin_location, "Apricot factory location が無い");
        ensure!(
            provider_ref.get_metadata.context("get_metadata が無い")?(
                raw,
                CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN,
                std::ptr::null(),
                &receiver
            ),
            "Apricot metadata に失敗"
        );
        drop(provider);
    }
    ensure!(data.error.is_none(), "Apricot discovery: {:?}", data.error);
    ensure!(!data.keys.is_empty(), "Apricot factory が空");
    Ok(data.keys.iter().map(|key| bundle.join(key)).collect())
}
