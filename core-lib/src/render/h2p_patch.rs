//! u-he の `.h2p` の音色切替（バイト列をそのまま CLAP state として流し込む）。
//!
//! TyrellN6・Zebralette3 の state は `.h2p` と同じテキスト形式で、`.h2p` を包み直さずに渡すと読む。
//! plugin は他製品の `.h2p` も拒否せず state を書き換えるので、送る前に `#AM=` 行で製品を確かめる
//! （[`crate::h2p::check_h2p_model`]）。
//! `process()` を回したあとの instance へ流した値は、その場では反映しきらず、数ブロックかけて
//! 新しい値へ寄っていく。空回しなしの 1 音目は前の音色寄りの音になるので、ロードのたびに
//! 空のブロックを回して寄せきってから返す（呼び出し側の settle に頼らない）。
//! まだ `process()` を回していない instance（`activate()` 前のロード）では遅れない。

use anyhow::{Context, Result};
use clack_host::prelude::{EventBuffer, PluginInstance};

use super::patch_state::load_plugin_state;
use super::RealtimeRenderer;
use crate::h2p::{check_h2p_model, h2p_product_of};
use crate::host::MidiRenderHost;
use cmrt_server_config::U_HE_PRODUCTS;

/// 反映を寄せきるのに要る空回しの frame 数。TyrellN6 で block の大きさを変えて測り、この frame 数と
/// [`STATE_SETTLE_MIN_BLOCKS`] の両方を満たすと包絡の差が 0.012 以下になった。
const STATE_SETTLE_MIN_FRAMES: usize = 4096;
/// 反映を寄せきるのに要る空回しのブロック数の下限（大きい block では frame 数だけでは足りない）。
const STATE_SETTLE_MIN_BLOCKS: usize = 4;

impl RealtimeRenderer {
    /// `.h2p` の音色を選ぶ。載っているプラグインが `.h2p` を読まない、または音色が別製品のものなら
    /// 送らずにエラーにする。
    pub(super) fn load_h2p_patch(&mut self, patch_path: &str) -> Result<()> {
        let plugin_id = self.plugin_id.clone();
        load_h2p_state(self.plugin_instance_mut(), &plugin_id, patch_path)?;
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

/// `.h2p` を読み、載っている製品の音色であることを確かめてから CLAP state として流し込む。
/// `activate()` 前にも呼べるよう自由関数にしてある。
pub(super) fn load_h2p_state(
    plugin_instance: &mut PluginInstance<MidiRenderHost>,
    plugin_id: &str,
    patch_path: &str,
) -> Result<()> {
    let product = ensure_h2p_capable(plugin_id)?;
    let bytes = std::fs::read(patch_path)
        .with_context(|| format!("{product} の音色ファイルを読めない '{patch_path}'"))?;
    check_h2p_model(product, &bytes).map_err(|error| anyhow::anyhow!("{error}: '{patch_path}'"))?;
    load_plugin_state(plugin_instance, &bytes)
        .with_context(|| format!("{product} の音色のロードに失敗 '{patch_path}'"))
}

/// `.h2p` を受け付けられるプラグインが載っているか。載っていればその製品名。
pub(super) fn ensure_h2p_capable(plugin_id: &str) -> Result<&'static str> {
    if let Some(product) = h2p_product_of(plugin_id) {
        return Ok(product);
    }
    let accepted = U_HE_PRODUCTS
        .iter()
        .map(|product| format!("'{}'", product.plugin_id))
        .collect::<Vec<_>>()
        .join(" / ");
    anyhow::bail!(
        "'.h2p' の音色は plugin_id = {accepted} でしか読めない（いま載っているのは '{plugin_id}'）"
    )
}

#[cfg(test)]
mod tests;
