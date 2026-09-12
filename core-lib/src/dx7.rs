//! Yamaha DX7 の 32-voice bulk dump（`.syx`）と、そこから作る patch 識別子。
//!
//! Dexed の音色は「1 cartridge = 4,104 bytes の `.syx` に 32 program」で配布され（Surge XT の
//! 1 音色 = 1 `.fxp` とは単位が違う）、Dexed は CLAP の preset 列挙/選択 API を opt-in していない
//! （`docs/adr/0006-no-generic-clap-preset-api.md`）ので host 側が自前で読む。
//! 公開されている DX7 SysEx 仕様に基づく独自実装で、Dexed（GPL）のコードは持ち込んでいない。

mod cartridge;
mod patch_path;
mod voice;

/// cartridge（DX7 の `.syx`）を音色置き場にする唯一の既知プラグインの CLAP plugin ID。
///
/// ロード直前に、patch 文字列の形が選んだプラグインと実際に載っているプラグインを照合する
/// （`docs/adr/0007-patch-string-decides-the-plugin.md`）。照合しないと Surge XT へ送った DX7 の SysEx が
/// 黙って無視され「操作は成功したのに音が変わらない」。`cmrt_server_config` の同名定数は profile の同定用。
pub const DEXED_PLUGIN_ID: &str = "com.digital-suburban.dexed";

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
