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

/// 相対パスで指定された音色（wire）を絶対パスへ直す。タグの無い旧形式はここでエラーにする。
///
/// 基点は 1 本ではなく patch 文字列の所有者ごとに違う。音色置き場がプラグインごとに
/// 別の場所にあるため（`docs/adr/0026-patch-tag-decides-the-owner.md`）。
pub(super) fn resolve_live_patch(
    patch: Option<String>,
    bases: &PatchBases,
) -> Result<Option<String>> {
    patch
        .filter(|patch| !patch.trim().is_empty())
        .map(|patch| bases.resolve(&patch))
        .transpose()
        .map_err(anyhow::Error::from)
}
