//! `set_patch()` — 要求されたパッチを、プラグインへの実際の操作へ翻訳して適用する。
//!
//! パッチの「形式」を知っているのは [`super::patch_state`]（`.fxp` と CLAP state）・
//! [`super::cartridge_patch`]（cartridge の 1 program）・[`super::vvp_patch`]（`.vvp`）で、
//! ここはそのどれを使うかを決めて、いま何が載っているかを記録するだけ。

use anyhow::Result;
use clack_host::prelude::PluginInstance;

use super::patch_state::{load_patch, load_plugin_state};
use super::RealtimeRenderer;
use crate::dx7::{is_cartridge_patch_path, parse_cartridge_patch_path, CartridgePatchPath};
use crate::host::MidiRenderHost;
use crate::vvp::is_vvp_patch_path;

impl RealtimeRenderer {
    /// 再生前にパッチを切り替える。同一パッチなら何もしない。
    /// `None` は生成直後にスナップショットした初期 state（Init Saw）へ戻す。
    /// state のロードは main-thread 操作のため、process() と直列なスレッドから呼ぶこと。
    ///
    /// cartridge patch（Dexed）だけは state load ではなく `process()` 経由の
    /// SysEx + Program Change になる（[`super::cartridge_patch`]）。`.vvp`（Vaporizer2）は
    /// Surge XT の `.fxp` と同じ state load だが、**渡す前に XML を包む**ので経路が別
    /// （[`super::vvp_patch`]）。
    pub fn set_patch(&mut self, patch: Option<&str>) -> Result<()> {
        if self.current_patch.as_deref() == patch {
            return Ok(());
        }
        match self.resolve_patch_target(patch)? {
            PatchTarget::Cartridge(cartridge) => self.load_cartridge_patch(&cartridge)?,
            PatchTarget::Vvp(path) => {
                self.forget_cartridge_program();
                self.load_vvp_patch(&path)?;
            }
            PatchTarget::StateFile(path) => {
                self.forget_cartridge_program();
                let plugin_instance = self.plugin_instance_mut();
                load_patch(plugin_instance, &path)?;
            }
            PatchTarget::InitState => {
                let init_state = self.init_state.take().ok_or_else(|| {
                    anyhow::anyhow!("初期 state が未取得のため初期音色へ戻せない")
                })?;
                self.forget_cartridge_program();
                let result = load_plugin_state(self.plugin_instance_mut(), &init_state);
                self.init_state = Some(init_state);
                result.map_err(|e| anyhow::anyhow!("初期 state の復元に失敗: {}", e))?;
            }
        }
        self.current_patch = patch.map(str::to_string);
        Ok(())
    }

    /// 要求されたパッチを、実際に行う操作へ翻訳する。
    ///
    /// `None` は Dexed でも Surge と同じく「生成直後の state へ戻す」でよい。
    /// 設計資料 7.3 は Dexed の `None` を初期 program へ正規化する方針だったが、
    /// これは cartridge + Program Change 方式で state load 直後の guard に当たるのを
    /// 避けるためのもの。single voice SysEx へ変えて guard と無関係になったので、
    /// 意味を曲げずに済むこちらを採る（[`cartridge_patch`]）。
    fn resolve_patch_target(&self, patch: Option<&str>) -> Result<PatchTarget> {
        let Some(path) = patch else {
            return Ok(PatchTarget::InitState);
        };
        if is_cartridge_patch_path(path) {
            Ok(PatchTarget::Cartridge(parse_cartridge_patch_path(path)?))
        } else if is_vvp_patch_path(path) {
            Ok(PatchTarget::Vvp(path.to_string()))
        } else {
            Ok(PatchTarget::StateFile(path.to_string()))
        }
    }

    pub(super) fn plugin_instance_mut(&mut self) -> &mut PluginInstance<MidiRenderHost> {
        self.plugin_instance
            .as_mut()
            .expect("plugin instance is always present while renderer is alive")
    }
}

/// `set_patch()` の要求を、プラグインへの実際の操作へ翻訳したもの。
enum PatchTarget {
    /// Dexed: cartridge SysEx + Program Change。
    Cartridge(CartridgePatchPath),
    /// Vaporizer2: `.vvp` の XML を JUCE binary-XML で包んで CLAP state としてロード。
    Vvp(String),
    /// Surge XT: `.fxp` を CLAP state としてロード。
    StateFile(String),
    /// 生成直後にスナップショットした state へ戻す。
    InitState,
}

#[cfg(test)]
mod tests;
