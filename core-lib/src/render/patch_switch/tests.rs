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
