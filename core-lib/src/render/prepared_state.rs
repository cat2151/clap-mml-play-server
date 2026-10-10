//! Owned state preparation through the existing native patch loaders.
use anyhow::Result;

use super::{save_plugin_state, RealtimeRenderer};
use crate::{load_entry, CoreConfig};

/// Creates, processes, snapshots and destroys a temporary renderer on the calling
/// thread. No device, GUI or server is created. The caller must exclude instance
/// creation in other hosts for this entire call; cmrt's creation lock is not a
/// lock shared with those hosts.
pub(crate) fn prepare_native_state(bundle: &str, id: &str, patch: &str) -> Result<Vec<u8>> {
    let entry = load_entry(bundle)?;
    let config = CoreConfig {
        plugin_id: Some(id.into()),
        patch_path: Some(patch.into()),
        sample_rate: 48_000.0,
        buffer_size: 256,
        ..Default::default()
    };
    let mut renderer = RealtimeRenderer::new(&config, &entry)?;
    // Dexed's loader processes its SysEx and settling block. Floe's loader waits
    // for pending changes and services callbacks before returning. SFZ state is
    // loaded before activation by the existing renderer constructor.
    if renderer
        .plugin_instance_mut()
        .access_shared_handler(|host| host.take_callback_request())
    {
        renderer
            .plugin_instance_mut()
            .call_on_main_thread_callback();
    }
    save_plugin_state(renderer.plugin_instance_mut())
}
