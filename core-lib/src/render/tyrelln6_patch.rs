//! TyrellN6 の音色切替（`.h2p` のバイト列をそのまま CLAP state として流し込む）。
//!
//! TyrellN6 の state は `.h2p` と同じテキスト形式で、`.h2p` を包み直さずに渡すと読む。
//! `process()` を回したあとの instance へ流した値は、その場では反映しきらず、数ブロックかけて
//! 新しい値へ寄っていく。空回しなしの 1 音目は前の音色寄りの音になるので、ロードのたびに
//! 空のブロックを回して寄せきってから返す（呼び出し側の settle に頼らない）。
//! まだ `process()` を回していない instance（`activate()` 前のロード）では遅れない。

use anyhow::{Context, Result};
use clack_host::prelude::{EventBuffer, PluginInstance};

use super::patch_state::load_plugin_state;
use super::RealtimeRenderer;
use crate::host::MidiRenderHost;
use crate::tyrelln6::TYRELLN6_PLUGIN_ID;

/// 反映を寄せきるのに要る空回しの frame 数。block の大きさを変えて測り、この frame 数と
/// [`STATE_SETTLE_MIN_BLOCKS`] の両方を満たすと包絡の差が 0.012 以下になった。
const STATE_SETTLE_MIN_FRAMES: usize = 4096;
/// 反映を寄せきるのに要る空回しのブロック数の下限（大きい block では frame 数だけでは足りない）。
const STATE_SETTLE_MIN_BLOCKS: usize = 4;

impl RealtimeRenderer {
    /// `.h2p` の音色を選ぶ。載っているプラグインが TyrellN6 でなければ送らずにエラーにする。
    pub(super) fn load_tyrelln6_patch(&mut self, patch_path: &str) -> Result<()> {
        ensure_tyrelln6_capable(&self.plugin_id)?;
        load_tyrelln6_state(self.plugin_instance_mut(), patch_path)?;
        let empty = EventBuffer::new();
        for _ in 0..state_settle_blocks(self.buf_size) {
            self.process_chunk_with_events(self.buf_size as u32, &empty)?;
        }
        Ok(())
    }
}

/// block の大きさ `buf_size` で、反映を寄せきるのに回す空のブロック数。
fn state_settle_blocks(buf_size: usize) -> usize {
    STATE_SETTLE_MIN_FRAMES
        .div_ceil(buf_size.max(1))
        .max(STATE_SETTLE_MIN_BLOCKS)
}

/// `.h2p` を読んで CLAP state として流し込む。`activate()` 前にも呼べるよう自由関数にしてある。
pub(super) fn load_tyrelln6_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    patch_path: &str,
) -> Result<()> {
    let bytes = std::fs::read(patch_path)
        .with_context(|| format!("TyrellN6 の音色ファイルを読めない '{patch_path}'"))?;
    load_plugin_state(plugin_instance, &bytes)
        .with_context(|| format!("TyrellN6 の音色のロードに失敗 '{patch_path}'"))
}

/// `.h2p` を受け付けられるプラグインが載っているか。
pub(super) fn ensure_tyrelln6_capable(plugin_id: &str) -> Result<()> {
    if plugin_id == TYRELLN6_PLUGIN_ID {
        return Ok(());
    }
    anyhow::bail!(
        "'.h2p' の音色は plugin_id = '{TYRELLN6_PLUGIN_ID}' でしか読めない（いま載っているのは '{plugin_id}'）"
    )
}

#[cfg(test)]
mod tests;
