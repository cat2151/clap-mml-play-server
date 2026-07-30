use std::path::Path;

use anyhow::Result;
use cmrt_realtime_ipc::{validate_instance_id, InstanceId};

pub(super) fn validate_live_instance_id(
    instance_id: InstanceId,
    live_instance_count: usize,
) -> Result<()> {
    validate_instance_id(instance_id)?;
    if usize::from(instance_id) >= live_instance_count {
        anyhow::bail!(
            "instance {instance_id} is outside configured live range 0..{live_instance_count}"
        );
    }
    Ok(())
}

pub(super) fn resolve_live_patch(
    patch: Option<String>,
    patches_dir: Option<&str>,
) -> Option<String> {
    patch
        .filter(|patch| !patch.trim().is_empty())
        .map(|patch| match patches_dir {
            Some(base) if !Path::new(&patch).is_absolute() => {
                Path::new(base).join(&patch).to_string_lossy().into_owned()
            }
            _ => patch,
        })
}
