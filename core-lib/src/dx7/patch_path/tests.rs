use super::*;

#[test]
fn detects_cartridge_patch_paths() {
    assert!(is_cartridge_patch_path(
        "SynprezFM/SynprezFM_01.syx/01 Say Again."
    ));
    assert!(is_cartridge_patch_path(
        r"C:\Cartridges\SynprezFM\SynprezFM_01.syx\00 Init"
    ));
    // program 番号が無くても cartridge 扱いにする（エラー文をこちら側で出すため）。
    assert!(is_cartridge_patch_path("SynprezFM_01.syx"));
    assert!(is_cartridge_patch_path("SynprezFM_01.SYX/00 Init"));
}

#[test]
fn surge_patch_paths_are_not_cartridge_paths() {
    assert!(!is_cartridge_patch_path("Pads/Pad 1.fxp"));
    assert!(!is_cartridge_patch_path(
        r"C:\Surge XT\patches_factory\Init.fxp"
    ));
    assert!(!is_cartridge_patch_path(""));
    // 拡張子だけのコンポーネントは cartridge ではない。
    assert!(!is_cartridge_patch_path(".syx/00 Init"));
}

#[test]
fn parses_slash_and_backslash_paths_the_same_way() {
    let slash = parse_cartridge_patch_path("SynprezFM/SynprezFM_01.syx/01 Say Again.").unwrap();
    assert_eq!(slash.cartridge_path, "SynprezFM/SynprezFM_01.syx");
    assert_eq!(slash.program_index, 1);

    let backslash =
        parse_cartridge_patch_path(r"C:\Cartridges\SynprezFM_01.syx\31 Last One").unwrap();
    assert_eq!(backslash.cartridge_path, r"C:\Cartridges\SynprezFM_01.syx");
    assert_eq!(backslash.program_index, 31);
}

#[test]
fn program_name_is_ignored_when_parsing() {
    let renamed = parse_cartridge_patch_path("Dexed_01.syx/07 まったく違う名前").unwrap();

    assert_eq!(renamed.program_index, 7);
}

#[test]
fn accepts_a_bare_two_digit_program_component() {
    let bare = parse_cartridge_patch_path("Dexed_01.syx/07").unwrap();

    assert_eq!(bare.program_index, 7);
}

#[test]
fn rejects_a_program_index_outside_the_cartridge() {
    let error = parse_cartridge_patch_path("Dexed_01.syx/32 Nope")
        .unwrap_err()
        .to_string();

    assert!(error.contains("32 Nope"), "{error}");
    assert!(error.contains("31"), "{error}");
}

#[test]
fn rejects_a_program_component_without_two_leading_digits() {
    for component in ["7 Seven", "Say Again.", "007 Bond", "1x Nope"] {
        let path = format!("Dexed_01.syx/{component}");
        let error = parse_cartridge_patch_path(&path).unwrap_err().to_string();
        assert!(error.contains(component), "{error}");
    }
}

#[test]
fn rejects_a_path_whose_syx_is_not_the_parent_component() {
    let error = parse_cartridge_patch_path("Dexed_01.syx/Sub/00 Init")
        .unwrap_err()
        .to_string();

    assert!(error.contains(".syx"), "{error}");
}

#[test]
fn rejects_a_cartridge_path_without_a_program_component() {
    let error = parse_cartridge_patch_path("Dexed_01.syx")
        .unwrap_err()
        .to_string();

    assert!(error.contains("program 番号"), "{error}");
}

#[test]
fn program_component_is_zero_based_and_two_digits() {
    assert_eq!(
        cartridge_program_component(0, "Say Again."),
        "00 Say Again."
    );
    assert_eq!(cartridge_program_component(31, "Last"), "31 Last");
}

#[test]
fn component_round_trips_through_the_parser() {
    for index in 0..DX7_PROGRAMS_PER_CARTRIDGE {
        let path = format!(
            "Dexed_01.syx/{}",
            cartridge_program_component(index, "Say Again.")
        );
        let parsed = parse_cartridge_patch_path(&path).unwrap();
        assert_eq!(usize::from(parsed.program_index), index);
    }
}
