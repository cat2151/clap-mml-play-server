//! `set_patch()` — 要求されたパッチを、プラグインへの実際の操作へ翻訳して適用する。
//!
//! パッチの「形式」を知っているのは [`super::patch_state`]（`.fxp` と CLAP state）・
//! [`super::cartridge_patch`]（cartridge の 1 program）・[`super::vvp_patch`]（`.vvp`）で、
//! ここはそのどれを使うかを決めて、いま何が載っているかを記録するだけ。
//!
//! 差し替えの**前後にどんな下準備をしてよいか**（鳴っている音を切る・反映のために
//! `process()` を空回しする）もここが持つ（[`RealtimeRenderer::switch_patch`]）。
//! 呼び出し側ごとに手順を組み立てると、**空回しが鳴っている音の再生位置を進める**という
//! 副作用を 1 か所で止められない。

use anyhow::Result;
use clack_host::prelude::PluginInstance;

use super::patch_state::{load_patch, load_plugin_state};
use super::RealtimeRenderer;
use crate::cache_wav::{cache_wav_state, is_cache_wav_patch_path};
use crate::dx7::{is_cartridge_patch_path, parse_cartridge_patch_path, CartridgePatchPath};
use crate::floe::is_floe_preset_path;
use crate::host::MidiRenderHost;
use crate::sforzando::is_sfz_patch_path;
use crate::vvp::is_vvp_patch_path;
use cmrt_cache_player::CACHE_PLAYER_PLUGIN_ID;

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
            PatchTarget::FloePreset(path) => {
                self.forget_cartridge_program();
                self.load_floe_preset(&path)?;
            }
            PatchTarget::Sfz(path) => {
                self.forget_cartridge_program();
                self.load_sfz_state(&path)?;
            }
            PatchTarget::CacheWav(path) => {
                self.forget_cartridge_program();
                let state = cache_wav_state(&path)?;
                load_plugin_state(self.plugin_instance_mut(), &state)
                    .map_err(|e| anyhow::anyhow!("キャッシュ WAV のロードに失敗 ({path}): {e}"))?;
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
        } else if is_sfz_patch_path(path) {
            Ok(PatchTarget::Sfz(path.to_string()))
        } else if is_floe_preset_path(path) {
            Ok(PatchTarget::FloePreset(path.to_string()))
        } else if is_vvp_patch_path(path) {
            Ok(PatchTarget::Vvp(path.to_string()))
        } else if is_cache_wav_patch_path(path) {
            Ok(PatchTarget::CacheWav(path.to_string()))
        } else {
            Ok(PatchTarget::StateFile(path.to_string()))
        }
    }

    /// 音色を差し替える。差し替えの前後の**下準備までを含んだ**入口。
    ///
    /// - `reset_before` … 載せる前に、鳴っている音を切って処理状態を捨てる
    /// - `settle_blocks` … 載せたあとに空回しする `process()` のブロック数。
    ///   state load だけでは反映されないプラグイン（Dexed）のために要る
    ///
    /// # 空回しは「鳴っている音の再生位置」を進める
    ///
    /// `reset()` の all sound off も settle の空回しも `process()` を呼ぶ。呼んだぶんだけ
    /// プラグインの中の時間は進むので、**鳴っている voice を持ったまま差し替えるプラグイン
    /// では、その音が空回しぶん先へ飛ぶ。**
    ///
    /// 実測（2026-09-03、`docs/adr/0018-patch-load-must-not-spin-the-plugin.md`）。DAW の先読みは
    /// 小節 N を鳴らしている最中に、**同じ instance の別スロットへ**小節 N+1 を載せる。
    /// cache-player は「鳴っている音はスロットの差し替えで切らない」契約なので、
    /// reset の 1 ブロックと settle の 4 ブロック、計 512×5 = 2560 フレーム（53.3ms）ぶん
    /// 鳴っている小節の再生位置が飛んでいた（小節の頭から 133ms 以内で 53ms 早くなり、
    /// そのぶん小節の終わりが鳴らずに終わる）。
    ///
    /// だから [`Self::keeps_voices_across_patch_load`] が真のプラグインでは
    /// **1 ブロックも回さない。** cache-player の state load は `load()` がスロットへ
    /// 差した時点で完了しているので、反映のための空回しも元から要らない。
    pub fn switch_patch(
        &mut self,
        patch: Option<&str>,
        reset_before: bool,
        settle_blocks: usize,
    ) -> Result<()> {
        let may_spin = !self.keeps_voices_across_patch_load();
        if reset_before && may_spin {
            self.reset();
        }
        self.set_patch(patch)?;
        if may_spin {
            for _ in 0..settle_blocks {
                self.render_live_chunk_with_offsets(&[])?;
            }
        }
        Ok(())
    }

    /// このプラグインは、音色を差し替えても**鳴っている音を切らない**契約か。
    ///
    /// 真なら、差し替えのついでに `process()` を回してはいけない
    /// （[`Self::switch_patch`] の「空回しは再生位置を進める」）。
    pub fn keeps_voices_across_patch_load(&self) -> bool {
        keeps_voices_across_patch_load(&self.plugin_id)
    }

    pub(super) fn plugin_instance_mut(&mut self) -> &mut PluginInstance<MidiRenderHost> {
        self.plugin_instance
            .as_mut()
            .expect("plugin instance is always present while renderer is alive")
    }
}

/// その plugin ID は「音色を差し替えても鳴っている音を切らない」プラグインか。
///
/// いまのところ組み込みの cache-player だけ。他のプラグインは差し替えのたびに
/// 鳴っている音を切ってよい（切らないと前の音色の voice が新しい state で鳴り続ける）。
fn keeps_voices_across_patch_load(plugin_id: &str) -> bool {
    plugin_id == CACHE_PLAYER_PLUGIN_ID
}

/// `set_patch()` の要求を、プラグインへの実際の操作へ翻訳したもの。
enum PatchTarget {
    /// Dexed: cartridge SysEx + Program Change。
    Cartridge(CartridgePatchPath),
    /// Vaporizer2: `.vvp` の XML を JUCE binary-XML で包んで CLAP state としてロード。
    Vvp(String),
    /// Floe: `.floe-preset` を Floe 固有 extension でロード。
    FloePreset(String),
    /// sforzando: 解決済み ARIA program を vendor state としてロード。
    Sfz(String),
    /// 組み込み cache-player: `.wav` の**パス**を CLAP state としてロード。
    ///
    /// ここだけ「ファイルの中身」ではなく「ファイルの場所」を state にする
    /// （理由は [`crate::cache_wav`]）。
    CacheWav(String),
    /// Surge XT: `.fxp` を CLAP state としてロード。
    StateFile(String),
    /// 生成直後にスナップショットした state へ戻す。
    InitState,
}

#[cfg(test)]
mod tests;
