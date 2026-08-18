//! 32-voice bulk dump（`.syx`）の検証と program 名の取り出し。
//!
//! レイアウト（実測。手元の 33 cartridge すべてがこの形）:
//!
//! | offset | 内容 |
//! |---|---|
//! | 0 | `0xF0` SysEx 開始 |
//! | 1 | `0x43` Yamaha |
//! | 2 | `0x0n` sub-status 0 + MIDI channel n |
//! | 3 | `0x09` format 9 = 32-voice bulk |
//! | 4-5 | `0x20 0x00` 続くデータ長 4096 を 7bit 2 バイトへ分けたもの |
//! | 6..4102 | packed voice 32 個（各 128 bytes） |
//! | 4102 | checksum |
//! | 4103 | `0xF7` SysEx 終了 |
//!
//! program 名は packed voice の末尾 10 bytes。

use anyhow::{bail, Result};

/// 32-voice bulk dump の総バイト数。この長さ以外は受け付けない。
pub const DX7_BULK_DUMP_LEN: usize = 4104;
/// 1 cartridge が持つ program 数。
pub const DX7_PROGRAMS_PER_CARTRIDGE: usize = 32;

const SYSEX_START: u8 = 0xF0;
const SYSEX_END: u8 = 0xF7;
const YAMAHA_MANUFACTURER_ID: u8 = 0x43;
/// sub-status（上位 nibble）が 0 = bulk dump。下位 nibble は MIDI channel なので問わない。
const SUB_STATUS_MASK: u8 = 0xF0;
/// format 9 = 32-voice bulk data。1 音色ぶんの format 0 とはここで区別する。
const FORMAT_32_VOICE: u8 = 0x09;
const BYTE_COUNT_MSB: u8 = 0x20;
const BYTE_COUNT_LSB: u8 = 0x00;

const HEADER_LEN: usize = 6;
pub(super) const PACKED_VOICE_LEN: usize = 128;
const NAME_OFFSET_IN_VOICE: usize = 118;
const NAME_LEN: usize = 10;
const DATA_LEN: usize = PACKED_VOICE_LEN * DX7_PROGRAMS_PER_CARTRIDGE;
const CHECKSUM_OFFSET: usize = HEADER_LEN + DATA_LEN;

/// 名前が空白と制御文字だけだったときの表示。空文字にすると patch path の
/// 末尾コンポーネントが `01 ` となり、区切り以外の情報が消えてしまう。
const UNNAMED_PROGRAM: &str = "(no name)";

/// 検証済みの 32-voice bulk dump。
///
/// `bytes` は読み込んだ内容そのままで、プラグインへはこれを 1 個の MIDI SysEx
/// event として送る。切り出しや詰め直しはしない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dx7Cartridge {
    bytes: Vec<u8>,
    program_names: Vec<String>,
}

impl Dx7Cartridge {
    /// プラグインへ送る SysEx バイト列（`0xF0` から `0xF7` まで）。
    pub fn sysex_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// 表示用に sanitize 済みの program 名 32 件。index は 0-based。
    pub fn program_names(&self) -> &[String] {
        &self.program_names
    }

    /// 1 program ぶんの packed voice（128 bytes）。
    ///
    /// # Panics
    /// `program_index` が cartridge の範囲外のとき。patch path のパーサが
    /// 範囲を保証しているので、ここへ来る時点では起こらない。
    pub(super) fn packed_voice(&self, program_index: u8) -> &[u8] {
        let start = HEADER_LEN + usize::from(program_index) * PACKED_VOICE_LEN;
        &self.bytes[start..start + PACKED_VOICE_LEN]
    }
}

/// `.syx` の内容を検証して cartridge にする。
///
/// 壊れたファイルを黙って通すと「SysEx は送れたのに音が変わらない」という
/// 一番切り分けにくい形で失敗するので、ここで全部弾く。
pub fn parse_dx7_cartridge(bytes: Vec<u8>) -> Result<Dx7Cartridge> {
    if bytes.len() != DX7_BULK_DUMP_LEN {
        bail!(
            "DX7 32-voice bulk dump は {} bytes でなければならない（実際は {} bytes）",
            DX7_BULK_DUMP_LEN,
            bytes.len()
        );
    }
    if bytes[0] != SYSEX_START || bytes[DX7_BULK_DUMP_LEN - 1] != SYSEX_END {
        bail!(
            "SysEx の開始/終了バイトが違う（先頭 0x{:02x}、末尾 0x{:02x}、期待は 0x{:02x} と 0x{:02x}）",
            bytes[0],
            bytes[DX7_BULK_DUMP_LEN - 1],
            SYSEX_START,
            SYSEX_END
        );
    }
    if bytes[1] != YAMAHA_MANUFACTURER_ID {
        bail!(
            "Yamaha の manufacturer ID ではない（0x{:02x}、期待は 0x{:02x}）",
            bytes[1],
            YAMAHA_MANUFACTURER_ID
        );
    }
    if bytes[2] & SUB_STATUS_MASK != 0 {
        bail!(
            "bulk dump ではない（sub-status 0x{:02x}）",
            bytes[2] & SUB_STATUS_MASK
        );
    }
    if bytes[3] != FORMAT_32_VOICE {
        bail!(
            "32-voice bulk format ではない（format 0x{:02x}、期待は 0x{:02x}）",
            bytes[3],
            FORMAT_32_VOICE
        );
    }
    if bytes[4] != BYTE_COUNT_MSB || bytes[5] != BYTE_COUNT_LSB {
        bail!(
            "データ長のヘッダが違う（0x{:02x} 0x{:02x}、期待は 0x{:02x} 0x{:02x}）",
            bytes[4],
            bytes[5],
            BYTE_COUNT_MSB,
            BYTE_COUNT_LSB
        );
    }
    let expected = checksum(&bytes[HEADER_LEN..CHECKSUM_OFFSET]);
    let actual = bytes[CHECKSUM_OFFSET];
    if expected != actual {
        bail!("checksum が合わない（0x{actual:02x}、期待は 0x{expected:02x}）");
    }

    let program_names = (0..DX7_PROGRAMS_PER_CARTRIDGE)
        .map(|index| {
            let start = HEADER_LEN + index * PACKED_VOICE_LEN + NAME_OFFSET_IN_VOICE;
            sanitize_program_name(&bytes[start..start + NAME_LEN])
        })
        .collect();
    Ok(Dx7Cartridge {
        bytes,
        program_names,
    })
}

/// DX7 の checksum は データ部の 7bit 総和の 2 の補数（下位 7bit）。
/// bulk dump と single voice dump で同じ式。
pub(super) fn checksum(data: &[u8]) -> u8 {
    let sum = data.iter().fold(0u32, |acc, byte| acc + u32::from(*byte)) & 0x7F;
    ((128 - sum) & 0x7F) as u8
}

/// 10 bytes 固定長の program 名を表示用の文字列にする。
///
/// この文字列は表示だけでなく patch path の一部＝永続 ID にもなるので、path として
/// 扱えない文字（区切り・制御文字）を残さない。重複名は DX7 では普通に起きるので許容する。
fn sanitize_program_name(raw: &[u8]) -> String {
    let name: String = raw
        .iter()
        .map(|byte| match byte {
            b'/' | b'\\' => ' ',
            0x20..=0x7E => *byte as char,
            _ => ' ',
        })
        .collect();
    let trimmed = name.trim();
    if trimmed.is_empty() {
        UNNAMED_PROGRAM.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 検証を通る最小の cartridge を組む。program 名だけ差し込める。
///
/// `.syx` を扱うテストが実物のファイルに依存しないよう、ここへ置いて
/// [`crate::patch_list`] のテストからも使う。
#[cfg(test)]
pub(crate) fn test_cartridge_bytes(names: &[(usize, &str)]) -> Vec<u8> {
    let mut bytes = vec![0u8; DX7_BULK_DUMP_LEN];
    bytes[0] = SYSEX_START;
    bytes[1] = YAMAHA_MANUFACTURER_ID;
    bytes[2] = 0x00;
    bytes[3] = FORMAT_32_VOICE;
    bytes[4] = BYTE_COUNT_MSB;
    bytes[5] = BYTE_COUNT_LSB;
    bytes[DX7_BULK_DUMP_LEN - 1] = SYSEX_END;
    for (index, name) in names {
        let start = HEADER_LEN + index * PACKED_VOICE_LEN + NAME_OFFSET_IN_VOICE;
        let raw = name.as_bytes();
        let len = raw.len().min(NAME_LEN);
        bytes[start..start + len].copy_from_slice(&raw[..len]);
    }
    bytes[CHECKSUM_OFFSET] = checksum(&bytes[HEADER_LEN..CHECKSUM_OFFSET]);
    bytes
}

#[cfg(test)]
mod tests;
