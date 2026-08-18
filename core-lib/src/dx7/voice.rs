//! packed voice（cartridge 内の 128 bytes）から single voice dump（163 bytes）を組む。
//!
//! # なぜ Program Change を使わないか
//! cartridge を送ってから MIDI Program Change で program を選ぶ手順は、**直前に CLAP
//! state load があると効かない**。Dexed v1.0.1 は state load の直後 約 2 秒、host からの
//! program change を無視するため。実測では `set_patch(None)`（= state load）の直後に
//! program 01 を選ぶと、cartridge の program 00 が鳴った。
//!
//! single voice dump は Dexed の edit buffer を直接書き換えるので、この guard と無関係。
//! `set_patch(None)` の意味（生成直後の初期音色へ戻す）も変えずに済む。
//!
//! # 形式（公開されている DX7 SysEx 仕様に基づく独自実装）
//! cartridge の voice は 128 bytes に詰められており、single voice dump は同じ内容を
//! 155 bytes へ展開したもの。詰め方は「1 バイトに 2〜3 個のフィールドを bit で押し込む」
//! だけで、operator は cartridge と同じ OP6→OP1 の順。

use super::cartridge::{checksum, Dx7Cartridge, PACKED_VOICE_LEN};

/// single voice dump の総バイト数。
pub const DX7_SINGLE_VOICE_DUMP_LEN: usize = 163;
/// 展開後の voice データ長。
const UNPACKED_VOICE_LEN: usize = 155;
const OPERATOR_COUNT: usize = 6;
const PACKED_OPERATOR_LEN: usize = 17;
const UNPACKED_OPERATOR_LEN: usize = 21;

const SYSEX_START: u8 = 0xF0;
const SYSEX_END: u8 = 0xF7;
const YAMAHA_MANUFACTURER_ID: u8 = 0x43;
/// format 0 = 1 voice（edit buffer 宛て）。cartridge の format 9 と対になる。
const FORMAT_SINGLE_VOICE: u8 = 0x00;
/// 続くデータ長 155 を 7bit 2 バイトへ分けたもの。
const BYTE_COUNT_MSB: u8 = 0x01;
const BYTE_COUNT_LSB: u8 = 0x1B;

/// cartridge の 1 program を、そのまま送れる single voice dump にする。
pub fn single_voice_sysex(cartridge: &Dx7Cartridge, program_index: u8) -> Vec<u8> {
    let unpacked = unpack_voice(cartridge.packed_voice(program_index));
    let mut sysex = Vec::with_capacity(DX7_SINGLE_VOICE_DUMP_LEN);
    sysex.extend_from_slice(&[
        SYSEX_START,
        YAMAHA_MANUFACTURER_ID,
        FORMAT_SINGLE_VOICE,
        FORMAT_SINGLE_VOICE,
        BYTE_COUNT_MSB,
        BYTE_COUNT_LSB,
    ]);
    sysex.extend_from_slice(&unpacked);
    sysex.push(checksum(&unpacked));
    sysex.push(SYSEX_END);
    debug_assert_eq!(sysex.len(), DX7_SINGLE_VOICE_DUMP_LEN);
    sysex
}

/// 128 bytes を 155 bytes へ展開する。
fn unpack_voice(packed: &[u8]) -> Vec<u8> {
    debug_assert_eq!(packed.len(), PACKED_VOICE_LEN);
    let mut out = Vec::with_capacity(UNPACKED_VOICE_LEN);
    for operator in 0..OPERATOR_COUNT {
        let src = &packed[operator * PACKED_OPERATOR_LEN..][..PACKED_OPERATOR_LEN];
        unpack_operator(src, &mut out);
    }
    unpack_global(&packed[OPERATOR_COUNT * PACKED_OPERATOR_LEN..], &mut out);
    debug_assert_eq!(out.len(), UNPACKED_VOICE_LEN);
    out
}

/// operator 1 個: 17 bytes -> 21 bytes。
fn unpack_operator(packed: &[u8], out: &mut Vec<u8>) {
    let before = out.len();
    // EG rate/level と keyboard level scaling の break point / depth はそのまま。
    out.extend_from_slice(&packed[0..11]);
    out.push(packed[11] & 0x03); // left curve
    out.push((packed[11] >> 2) & 0x03); // right curve
    out.push(packed[12] & 0x07); // rate scaling
    out.push(packed[13] & 0x03); // amplitude modulation sensitivity
    out.push((packed[13] >> 2) & 0x07); // key velocity sensitivity
    out.push(packed[14]); // output level
    out.push(packed[15] & 0x01); // oscillator mode
    out.push((packed[15] >> 1) & 0x1F); // frequency coarse
    out.push(packed[16]); // frequency fine
    out.push((packed[12] >> 3) & 0x0F); // detune
    debug_assert_eq!(out.len() - before, UNPACKED_OPERATOR_LEN);
}

/// voice 共通部: 26 bytes -> 29 bytes。
fn unpack_global(packed: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&packed[0..8]); // pitch EG rate 1-4 / level 1-4
    out.push(packed[8]); // algorithm
    out.push(packed[9] & 0x07); // feedback
    out.push((packed[9] >> 3) & 0x01); // oscillator key sync
    out.extend_from_slice(&packed[10..14]); // LFO speed / delay / pitch mod / amp mod
    out.push(packed[14] & 0x01); // LFO key sync
    out.push((packed[14] >> 1) & 0x07); // LFO waveform
    out.push((packed[14] >> 4) & 0x07); // pitch modulation sensitivity
    out.push(packed[15]); // transpose
    out.extend_from_slice(&packed[16..26]); // name (10 bytes)
}

#[cfg(test)]
mod tests;
