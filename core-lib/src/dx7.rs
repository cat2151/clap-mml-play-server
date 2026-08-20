//! Yamaha DX7 の 32-voice bulk dump（`.syx`）と、そこから作る patch 識別子。
//!
//! Dexed の音色は「1 cartridge = 4,104 bytes の `.syx` に 32 program」という形で
//! 配布される。Surge XT の「1 音色 = 1 `.fxp` = 1 CLAP state」とは単位が違うので、
//! patch path の解釈とロード手順をここへ分けている。
//!
//! CLAP には preset を列挙・選択する専用 API（preset-discovery factory と
//! `clap.preset-load` extension）があるが、Dexed v1.0.1 はどちらも opt-in して
//! いない（実測。`docs/adr/0006-no-generic-clap-preset-api.md`）。したがって host 側が
//! この形式を自前で読むしかない。
//!
//! 実装は公開されている DX7 SysEx 仕様に基づく独自実装で、Dexed（GPL）の
//! コードは持ち込んでいない。

mod cartridge;
mod patch_path;
mod voice;

/// cartridge（DX7 の `.syx`）を音色置き場にする唯一の既知プラグインの CLAP plugin ID。
///
/// ロード経路が「patch 文字列の形」で分岐する（[`is_cartridge_patch_path`]）以上、
/// **選ばれた形と実際に載っているプラグインが食い違っていないか**をロード直前に
/// 照合する必要がある。照合しないと、Surge XT のインスタンスへ DX7 の SysEx を送っても
/// 黙って無視されて「操作は成功したのに音が変わらない」状態になる
/// （`docs/adr/0007-patch-string-decides-the-plugin.md`）。その照合に使う。
///
/// `cmrt_server_config` にも同じ定数がある（config の `active_plugin` 解決用）。
/// core-lib は server-config へ依存しているので技術的には寄せられるが、
/// 用途が別（こちらは「載っているプラグインの照合」、あちらは「config の解決」）なので
/// 二重に持っている。寄せるなら両方の用途を 1 つの定数で説明できるか先に確かめること。
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
