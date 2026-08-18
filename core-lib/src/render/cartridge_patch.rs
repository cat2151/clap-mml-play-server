//! Dexed の音色切替（cartridge の 1 program を single voice SysEx で送る）。
//!
//! Surge XT の `.fxp` は CLAP state として同期的に流し込めるが、Dexed の cartridge は
//! **`process()` を通さないと届かない**。SysEx が event だからで、そのぶん手順が
//! state load より長い。
//!
//! # なぜ「cartridge を送って Program Change」ではないか
//! 報告書（`DEXED_CLAP_SUPPORT_REPORT.md` 5 章）で実証されたのは
//! 「4,104 bytes の cartridge を送ってから `[0xC0|ch, program, 0]` を送る」手順だが、
//! **直前に CLAP state load があると Program Change だけが捨てられる**。Dexed v1.0.1 は
//! state load の直後 約 2 秒 program change を無視するためで、`set_patch(None)` は
//! まさに state load。実測では `None` の直後に program 01 を選ぶと program 00 が鳴った。
//!
//! そこで cartridge から目的の voice だけを取り出し、**1 voice ぶんの SysEx**
//! （163 bytes、[`crate::dx7::single_voice_sysex`]）として送る。Dexed の edit buffer を
//! 直接書き換えるので program change の guard と無関係になり、`set_patch(None)` の
//! 意味（生成直後の初期音色へ戻す）も Surge と揃ったまま変えずに済む。

use anyhow::{Context, Result};
use clack_host::events::event_types::MidiSysExEvent;
use clack_host::prelude::EventBuffer;

use super::RealtimeRenderer;
use crate::dx7::{parse_dx7_cartridge, single_voice_sysex, CartridgePatchPath, Dx7Cartridge};

/// voice を送ったあとに空回しするブロック数。切替直後の 1 音目を取りこぼさないため。
const VOICE_SETTLE_BLOCKS: usize = 1;

impl RealtimeRenderer {
    /// cartridge の 1 program を選ぶ。
    pub(super) fn load_cartridge_patch(&mut self, patch: &CartridgePatchPath) -> Result<()> {
        if self.loaded_program.as_ref()
            == Some(&(patch.cartridge_path.clone(), patch.program_index))
        {
            return Ok(());
        }
        self.silence_all_notes();
        self.send_voice(patch)?;

        let empty = EventBuffer::new();
        for _ in 0..VOICE_SETTLE_BLOCKS {
            self.process_chunk_with_events(self.buf_size as u32, &empty)?;
        }
        Ok(())
    }

    /// state load や `.fxp` の読み込みで、プラグインに載っている voice が
    /// cartridge 由来でなくなったことを記録する。
    pub(super) fn forget_cartridge_program(&mut self) {
        self.loaded_program = None;
    }

    fn send_voice(&mut self, patch: &CartridgePatchPath) -> Result<()> {
        // 途中で失敗したら「何が載っているか分からない」状態なので、先に忘れておく。
        self.forget_cartridge_program();
        let sysex = {
            let cartridge = self.cartridge(&patch.cartridge_path)?;
            single_voice_sysex(cartridge, patch.program_index)
        };

        // SAFETY: MidiSysExEvent は buffer を借用するだけなので、`process()` が返るまで
        // `sysex` を生かしておく必要がある。この関数を抜けるまで drop されない。
        let event = unsafe { MidiSysExEvent::new(0, 0, &sysex) };
        let mut events = EventBuffer::new();
        events.push(&event);
        let result = self
            .process_chunk_with_events(self.buf_size as u32, &events)
            .with_context(|| {
                format!(
                    "program {} の送信に失敗 '{}'",
                    patch.program_index, patch.cartridge_path
                )
            });
        drop(sysex);
        result?;

        self.loaded_program = Some((patch.cartridge_path.clone(), patch.program_index));
        Ok(())
    }

    /// cartridge を読む。同じ cartridge の別 program へ移るときは読み直さない。
    ///
    /// `set_patch()` は再生サーバーの worker スレッドから呼ばれるので、
    /// ファイル読み込みと 4KB のパースを毎回やらせない。
    fn cartridge(&mut self, cartridge_path: &str) -> Result<&Dx7Cartridge> {
        if self
            .cartridge_cache
            .as_ref()
            .is_none_or(|(path, _)| path != cartridge_path)
        {
            let bytes = std::fs::read(cartridge_path)
                .with_context(|| format!("cartridge を読めない '{cartridge_path}'"))?;
            let cartridge = parse_dx7_cartridge(bytes)
                .with_context(|| format!("cartridge として読めない '{cartridge_path}'"))?;
            self.cartridge_cache = Some((cartridge_path.to_string(), cartridge));
        }
        Ok(&self
            .cartridge_cache
            .as_ref()
            .expect("cartridge cache was just filled")
            .1)
    }
}

#[cfg(test)]
mod tests;
