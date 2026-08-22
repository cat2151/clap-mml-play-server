//! Vaporizer2 の音色切替（`.vvp` を CLAP state として流し込む）。
//!
//! Surge XT の `.fxp` と同じく **state load で完結する**（`process()` は要らない）。
//! Dexed の cartridge が SysEx の event である（[`super::cartridge_patch`]）のとは
//! そこが違う。ファイル形式の変換は [`crate::vvp`] が持ち、ここは
//! 「載っているプラグインが受け付けられるか」の照合とロードだけを行う。

use anyhow::{Context, Result};
use clack_host::prelude::PluginInstance;

use super::patch_state::load_plugin_state;
use super::RealtimeRenderer;
use crate::host::MidiRenderHost;
use crate::vvp::{vvp_state_blob, VAPORIZER2_PLUGIN_ID};

impl RealtimeRenderer {
    /// `.vvp` の音色を選ぶ。
    ///
    /// 載っているプラグインが `.vvp` を解さないなら、送らずにエラーにする。
    /// Dexed の SysEx と同じ理由で、**送ってしまうと静かに間違う**
    /// （`docs/adr/0007-patch-string-decides-the-plugin.md`）。`.vvp` の場合は
    /// Surge XT へ「知らない形の state」を流し込むことになるので、無視されるだけでは
    /// 済まない可能性もある。
    pub(super) fn load_vvp_patch(&mut self, patch_path: &str) -> Result<()> {
        ensure_vvp_capable(&self.plugin_id)?;
        load_vvp_state(self.plugin_instance_mut(), patch_path)
    }
}

/// `.vvp` を読んで CLAP state として流し込む。
///
/// [`RealtimeRenderer`] を組み立てる前（`activate()` 前）にも音色を載せられるよう、
/// メソッドではなく自由関数にしてある。`.vvp` は state なので Surge XT の `.fxp` と同じく
/// activate 前でよい（cartridge の SysEx は event なので送れない、というのが
/// [`super::cartridge_patch`] との違い）。
pub(super) fn load_vvp_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    patch_path: &str,
) -> Result<()> {
    let xml = std::fs::read(patch_path)
        .with_context(|| format!("音色ファイルを読めない '{patch_path}'"))?;
    let blob = vvp_state_blob(&xml);
    load_plugin_state(plugin_instance, &blob)
        .with_context(|| format!("音色のロードに失敗 '{patch_path}'"))
}

/// `.vvp` を受け付けられるプラグインが載っているか。
pub(super) fn ensure_vvp_capable(plugin_id: &str) -> Result<()> {
    if plugin_id == VAPORIZER2_PLUGIN_ID {
        return Ok(());
    }
    anyhow::bail!(
        "'.vvp' の音色は plugin_id = '{VAPORIZER2_PLUGIN_ID}' でしか読めない（いま載っているのは '{plugin_id}'）"
    )
}
