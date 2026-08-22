use super::*;

/// パッチ文字列だけで行き先が決まること。`RealtimeRenderer` を作らずに確かめられる部分。
#[test]
fn cartridge_paths_and_state_files_are_told_apart_by_the_path_alone() {
    assert!(is_cartridge_patch_path("Dexed_01.syx/00 Init"));
    assert!(!is_cartridge_patch_path("Pads/Pad 1.fxp"));
}

#[test]
fn a_cartridge_path_resolves_to_its_cartridge_and_program() {
    let parsed = parse_cartridge_patch_path("Dexed_01.syx/07 Seven").unwrap();

    assert_eq!(
        parsed,
        CartridgePatchPath {
            cartridge_path: "Dexed_01.syx".to_string(),
            program_index: 7,
        }
    );
}

/// 3 つ目の形が増えても、判定の順番が入れ替わって取り違えないこと。
#[test]
fn vvp_paths_are_told_apart_from_both_other_forms_by_the_path_alone() {
    assert!(is_vvp_patch_path("AR Accent Arp.vvp"));
    assert!(is_vvp_patch_path("User/PD Emily.VVP"));
    assert!(!is_vvp_patch_path("Pads/Pad 1.fxp"));
    assert!(!is_vvp_patch_path("Dexed_01.syx/00 Init"));
    assert!(!is_cartridge_patch_path("AR Accent Arp.vvp"));
    // 拡張子だけの名前は音色ファイルではない。
    assert!(!is_vvp_patch_path(".vvp"));
}
