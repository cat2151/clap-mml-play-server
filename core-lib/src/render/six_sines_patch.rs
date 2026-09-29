//! Six Sines の音色切替（`.sxsnp` のバイト列をそのまま CLAP state として流し込む）。
//!
//! Six Sines の `stateSave` は patch の XML に NUL 1 byte を足しただけを書き、`stateLoad` は
//! NUL まで（無ければ末尾まで）を XML として読む。`.sxsnp` は同じ XML なので包み直さない。
//! 流し込んだ値は audio thread 向けのキューに積まれ、次の `process()` の先頭で反映される。
//! 反映のときに鳴っている音を全部止めるので、**反映と同じブロックの note-on は鳴らない。**
//! だからロードのたびに空のブロックを回して反映を済ませてから返す（呼び出し側の settle に頼らない）。

use anyhow::{Context, Result};
use clack_host::prelude::{EventBuffer, PluginInstance};

use super::patch_state::load_plugin_state;
use super::RealtimeRenderer;
use crate::host::MidiRenderHost;
use crate::six_sines::SIX_SINES_PLUGIN_ID;

/// state を流したあとに回す空のブロック数。キューは 1 回の `process()` で全件捌かれる。
const STATE_APPLY_BLOCKS: usize = 1;

impl RealtimeRenderer {
    /// `.sxsnp` の音色を選ぶ。載っているプラグインが Six Sines でなければ送らずにエラーにする。
    pub(super) fn load_six_sines_patch(&mut self, patch_path: &str) -> Result<()> {
        ensure_six_sines_capable(&self.plugin_id)?;
        load_six_sines_state(self.plugin_instance_mut(), patch_path)?;
        self.apply_six_sines_state()
    }

    /// 流し込み済みの state を、空のブロックを回して反映させる。
    pub(super) fn apply_six_sines_state(&mut self) -> Result<()> {
        let empty = EventBuffer::new();
        for _ in 0..STATE_APPLY_BLOCKS {
            self.process_chunk_with_events(self.buf_size as u32, &empty)?;
        }
        Ok(())
    }
}

/// `.sxsnp` を読んで CLAP state として流し込む。`activate()` 前にも呼べるよう自由関数にしてある。
pub(super) fn load_six_sines_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    patch_path: &str,
) -> Result<()> {
    let xml = std::fs::read(patch_path)
        .with_context(|| format!("Six Sines の音色ファイルを読めない '{patch_path}'"))?;
    load_plugin_state(plugin_instance, &xml)
        .with_context(|| format!("Six Sines の音色のロードに失敗 '{patch_path}'"))
}

/// `.sxsnp` を受け付けられるプラグインが載っているか。
pub(super) fn ensure_six_sines_capable(plugin_id: &str) -> Result<()> {
    if plugin_id == SIX_SINES_PLUGIN_ID {
        return Ok(());
    }
    anyhow::bail!(
        "'.sxsnp' の音色は plugin_id = '{SIX_SINES_PLUGIN_ID}' でしか読めない（いま載っているのは '{plugin_id}'）"
    )
}
