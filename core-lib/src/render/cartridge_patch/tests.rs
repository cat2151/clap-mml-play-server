//! 実プラグインを起動しないで確かめられる範囲。
//!
//! 実際に Dexed へ voice を送って音が変わることの確認は、[`super::super::tests`] の
//! `#[ignore]` 付き統合テストで行う。

use super::*;

/// cache は「ファイルを読み直さないため」だけのもので、プラグインの状態とは無関係。
/// 同じ cartridge の別 program へ移っても読み直さないことをここで示す。
#[test]
fn the_cartridge_cache_key_is_the_cartridge_path_not_the_program() {
    let first = CartridgePatchPath {
        cartridge_path: "Dexed_01.syx".to_string(),
        program_index: 0,
    };
    let second = CartridgePatchPath {
        cartridge_path: "Dexed_01.syx".to_string(),
        program_index: 31,
    };

    assert_eq!(first.cartridge_path, second.cartridge_path);
}
