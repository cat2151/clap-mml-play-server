use std::path::Path;

use anyhow::Result;
use cmrt_realtime_ipc::{validate_instance_id, InstanceId};

use super::instances::PatchBases;

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

/// 相対パスで指定された音色を絶対パスへ直す。
///
/// 基点は 1 本ではなく patch 文字列の形ごとに違う。音色置き場がプラグインごとに
/// 別の場所にあるため（`docs/adr/0007-patch-string-decides-the-plugin.md`）。
pub(super) fn resolve_live_patch(patch: Option<String>, bases: &PatchBases) -> Option<String> {
    patch
        .filter(|patch| !patch.trim().is_empty())
        .map(|patch| match bases.base_for(&patch) {
            Some(base) if !Path::new(&patch).is_absolute() => {
                Path::new(base).join(&patch).to_string_lossy().into_owned()
            }
            _ => patch,
        })
}
