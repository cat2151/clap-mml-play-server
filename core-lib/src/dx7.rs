//! Yamaha DX7 の 32-voice bulk dump（`.syx`）と、そこから作る patch 識別子。
//!
//! Dexed の音色は「1 cartridge = 4,104 bytes の `.syx` に 32 program」という形で
//! 配布される。Surge XT の「1 音色 = 1 `.fxp` = 1 CLAP state」とは単位が違うので、
//! patch path の解釈とロード手順をここへ分けている。
//!
//! CLAP には preset を列挙・選択する専用 API（preset-discovery factory と
//! `clap.preset-load` extension）があるが、Dexed v1.0.1 はどちらも opt-in して
//! いない（実測。`DEXED_CLAP_SUPPORT_REPORT.md` 5 章）。したがって host 側が
//! この形式を自前で読むしかない。
//!
//! 実装は公開されている DX7 SysEx 仕様に基づく独自実装で、Dexed（GPL）の
//! コードは持ち込んでいない。

mod cartridge;
mod patch_path;
mod voice;

#[cfg(test)]
pub(crate) use cartridge::test_cartridge_bytes;
pub use cartridge::{
    parse_dx7_cartridge, Dx7Cartridge, DX7_BULK_DUMP_LEN, DX7_PROGRAMS_PER_CARTRIDGE,
};
pub use patch_path::{
    cartridge_program_component, is_cartridge_patch_path, parse_cartridge_patch_path,
    CartridgePatchPath,
};
pub use voice::{single_voice_sysex, DX7_SINGLE_VOICE_DUMP_LEN};
