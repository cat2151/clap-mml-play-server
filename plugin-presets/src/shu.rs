//! Shu（Mikey Audio のシマーリバーブ）の Algorithm ごとの preset を plugin state に組み立てる。
//!
//! Shu は preset を持たないので、Algorithm（7 種）を 1 件ずつの preset として host 側で定義する。
//! state は param id 順に 35 個の f64（リトルエンディアン）を並べた物で、値は param の
//! 正規化値ではなく表示単位（Decay の 0.5 は 50.0）。init state を雛形にして、
//! Algorithm（と Decay）の slot だけを書き換える（[`shu_state_blob`]）。
//! Algorithm を切り替えても plugin は他の param を連動させないので、残りは init 値のままでよい。

use anyhow::{bail, Result};

use crate::effect_preset::PresetValue;

pub const SHU_PLUGIN_ID: &str = "audio.mikey.Shu";

/// state の slot 数（= param 数）。
pub const SHU_PARAM_COUNT: usize = 35;
const SLOT_BYTES: usize = 8;
pub const SHU_DECAY_SLOT: usize = 1;
pub const SHU_ALGORITHM_SLOT: usize = 4;
/// Decay を最大にしたときの slot の値（表示単位の 100 %）。
const DECAY_MAX_SLOT_VALUE: f64 = 100.0;

pub const SHU_CATEGORY: &str = "Space / Imaging";
const KIND_SHIMMER_REVERB: &str = "Shimmer Reverb";
const KIND_REVERB: &str = "Reverb";

#[derive(Debug)]
pub struct ShuPreset {
    pub name: &'static str,
    /// Algorithm param の値（0=Silk … 6=Ether）。
    pub algorithm: u8,
    pub decay_max: bool,
    pub kind: &'static str,
}

const ETHER: u8 = 6;

pub const SHU_PRESETS: [ShuPreset; 8] = [
    reverb("Silk", 0),
    reverb("Drift", 1),
    reverb("Plate", 2),
    reverb("Classic", 3),
    reverb("Slap", 4),
    reverb("Spring", 5),
    ShuPreset {
        name: "Ether",
        algorithm: ETHER,
        decay_max: false,
        kind: KIND_SHIMMER_REVERB,
    },
    ShuPreset {
        name: "Ether Decay Max",
        algorithm: ETHER,
        decay_max: true,
        kind: KIND_SHIMMER_REVERB,
    },
];

const fn reverb(name: &'static str, algorithm: u8) -> ShuPreset {
    ShuPreset {
        name,
        algorithm,
        decay_max: false,
        kind: KIND_REVERB,
    }
}

pub fn shu_preset(name: &str) -> Result<&'static ShuPreset> {
    match SHU_PRESETS.iter().find(|preset| preset.name == name) {
        Some(preset) => Ok(preset),
        None => bail!("Shu に preset '{name}' は無い"),
    }
}

/// init state を雛形に、preset を載せた state を組む。長さが 35 slot でなければエラー
/// （plugin の版が違う。黙って別の slot を書き換えない）。
pub fn shu_state_blob(init_state: &[u8], preset: &ShuPreset) -> Result<Vec<u8>> {
    if init_state.len() != SHU_PARAM_COUNT * SLOT_BYTES {
        bail!(
            "Shu の state が {} bytes（{} bytes のはず）",
            init_state.len(),
            SHU_PARAM_COUNT * SLOT_BYTES
        );
    }
    let mut state = init_state.to_vec();
    write_slot(&mut state, SHU_ALGORITHM_SLOT, f64::from(preset.algorithm));
    if preset.decay_max {
        write_slot(&mut state, SHU_DECAY_SLOT, DECAY_MAX_SLOT_VALUE);
    }
    Ok(state)
}

fn write_slot(state: &mut [u8], slot: usize, value: f64) {
    state[slot * SLOT_BYTES..(slot + 1) * SLOT_BYTES].copy_from_slice(&value.to_le_bytes());
}

/// 組み込み preset を一覧の値にする。preset 名がそのまま値と表示名になる。
pub fn shu_preset_values() -> Vec<PresetValue> {
    SHU_PRESETS
        .iter()
        .map(|preset| PresetValue {
            value: preset.name.to_string(),
            shown: preset.name.to_string(),
            category: SHU_CATEGORY.to_string(),
            kind: preset.kind.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
