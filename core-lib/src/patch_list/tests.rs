use super::*;

mod installed;

#[test]
fn collect_patches_lists_only_floe_presets_from_a_floe_library() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_floe");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let bank = tmp_dir.join("Taiko Drums Factory Presets");
    let details = tmp_dir.join("Floe-Details");
    std::fs::create_dir_all(&bank).unwrap();
    std::fs::create_dir_all(&details).unwrap();
    std::fs::write(bank.join("Taiko Beat.floe-preset"), b"state").unwrap();
    std::fs::write(bank.join("Taiko Beat 2.FLOE-PRESET"), b"state").unwrap();
    std::fs::write(tmp_dir.join("Library.floe-pkg"), b"package").unwrap();
    std::fs::write(tmp_dir.join("floe-preset-bank.ini"), b"ini").unwrap();
    std::fs::write(details.join("checksums.crc32"), b"crc").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    assert_eq!(patches.len(), 2);
    assert!(patches
        .iter()
        .all(|path| crate::is_floe_preset_path(&path.to_string_lossy())));
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn collect_patches_lists_sfz_case_insensitively_and_ignores_other_files() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_sfz");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let bank = tmp_dir.join("Garritan");
    std::fs::create_dir_all(&bank).unwrap();
    std::fs::write(bank.join("Glockenspiel.sfz"), b"<region>").unwrap();
    std::fs::write(bank.join("Piano.SFZ"), b"<region>").unwrap();
    std::fs::write(bank.join("notes.txt"), b"not a patch").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    assert_eq!(patches.len(), 2);
    assert!(patches
        .iter()
        .all(|path| crate::is_sfz_patch_path(&path.to_string_lossy())));
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

/// `.h2p` は 1 ファイル = 1 音色。中身に NUL を含んでも開かずに列挙する。
#[test]
fn collect_patches_lists_tyrelln6_presets_under_category_dirs() {
    let tmp_dir = std::env::temp_dir().join("cmrt_test_collect_patches_h2p");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let basses = tmp_dir.join("01 Basses");
    std::fs::create_dir_all(&basses).unwrap();
    std::fs::write(basses.join("Abgrund.h2p"), b"#AM=TyrellN6\n\0\0").unwrap();
    std::fs::write(basses.join("Loud.H2P"), b"#AM=TyrellN6\n\0\0").unwrap();
    std::fs::write(tmp_dir.join("Midi.Bank.Cache.txt"), b"not a patch").unwrap();

    let patches = collect_patches(tmp_dir.to_str().unwrap()).unwrap();

    assert_eq!(patches.len(), 2);
    assert!(patches
        .iter()
        .all(|path| crate::is_tyrelln6_patch_path(&path.to_string_lossy())));
    assert_eq!(
        to_relative(tmp_dir.to_str().unwrap(), &patches[0]),
        "01 Basses/Abgrund.h2p"
    );
    let _ = std::fs::remove_dir_all(&tmp_dir);
}
