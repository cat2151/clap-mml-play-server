use super::*;

const ALL_FORMS: [PatchForm; 10] = [
    PatchForm::StateFile,
    PatchForm::Cartridge,
    PatchForm::Vvp,
    PatchForm::FloePreset,
    PatchForm::Sfz,
    PatchForm::SixSines,
    PatchForm::H2p,
    PatchForm::Jc303Preset,
    PatchForm::CacheWav,
    PatchForm::ApricotPreset,
];

/// variant を足したら `ALL_FORMS` も足させるための網羅 match。
fn index_of(form: PatchForm) -> usize {
    match form {
        PatchForm::StateFile => 0,
        PatchForm::Cartridge => 1,
        PatchForm::Vvp => 2,
        PatchForm::FloePreset => 3,
        PatchForm::Sfz => 4,
        PatchForm::SixSines => 5,
        PatchForm::H2p => 6,
        PatchForm::Jc303Preset => 7,
        PatchForm::CacheWav => 8,
        PatchForm::ApricotPreset => 9,
    }
}

#[test]
fn every_form_has_exactly_one_default_owner() {
    for (i, form) in ALL_FORMS.iter().enumerate() {
        assert_eq!(index_of(*form), i);
        let owners = DEFAULT_OWNERS.iter().filter(|r| r.form == *form).count();
        assert_eq!(owners, 1, "{form:?}");
    }
    assert_eq!(DEFAULT_OWNERS.len(), ALL_FORMS.len());
    assert!(DEFAULT_OWNERS.iter().all(|r| r.tag.is_empty()));
}

#[test]
fn extra_tags_are_non_empty_valid_and_unique_per_form() {
    for row in EXTRA_TAGS {
        assert!(!row.tag.is_empty() && row.tag.len() <= MAX_PATCH_TAG_LEN);
        assert!(row.tag.bytes().all(|b| b.is_ascii_alphanumeric()));
        let same = rows()
            .filter(|r| r.form == row.form && r.tag == row.tag)
            .count();
        assert_eq!(same, 1, "{row:?}");
    }
}

#[test]
fn longer_tags_require_all_shorter_tags_in_the_same_form_to_be_used() {
    let alphabet_size = u128::from(26_u8 + 26 + 10);
    for row in EXTRA_TAGS {
        for length in 1..row.tag.len() {
            let used = EXTRA_TAGS
                .iter()
                .filter(|other| other.form == row.form && other.tag.len() == length)
                .count() as u128;
            assert_eq!(
                used,
                alphabet_size.pow(length as u32),
                "{} のタグ '{}' は、同じ拡張子で {length} 文字の候補を使い切る前に長くしている",
                row.owner,
                row.tag
            );
        }
    }
}

fn owner(wire: &str) -> &'static str {
    parse_patch_wire(wire).unwrap().owner
}

#[test]
fn fxp_owners_and_display_round_trip_are_distinct() {
    assert_eq!(owner("Basses/Bass.fxp:"), "Surge XT");
    let wire = "Basses/Bass.fxp:o";
    let parsed = parse_patch_wire(wire).unwrap();
    assert_eq!(parsed.owner, "OB-Xf");
    assert_eq!(parsed.path, "Basses/Bass.fxp");
    assert_eq!(wire_from_display(&display_from_wire(&parsed)), wire);
    assert_eq!(split_display(wire), ("Basses/Bass.fxp", "o"));
    assert!(matches!(
        parse_patch_wire("Basses/Bass.fxp:obxf"),
        Err(PatchWireError::UnknownTag { .. })
    ));
}

#[test]
fn untagged_patches_are_legacy_errors() {
    for wire in ["x.h2p", r"D:\d\x.h2p", "Pads/Pad 1.fxp", "D:/d/x.fxp"] {
        assert!(
            matches!(parse_patch_wire(wire), Err(PatchWireError::Legacy { .. })),
            "{wire}"
        );
    }
}

#[test]
fn explicit_default_owner_tag_is_rejected_with_the_canonical_form() {
    let err = parse_patch_wire("x.h2p:t").unwrap_err();
    assert!(matches!(err, PatchWireError::UnknownTag { .. }));
    assert!(err.to_string().contains("TyrellN6 は ':'"), "{err}");
}

#[test]
fn tags_are_case_sensitive_alphanumeric_and_bounded() {
    assert!(matches!(
        parse_patch_wire("x.h2p:Z"),
        Err(PatchWireError::UnknownTag { .. })
    ));
    let long = format!("x.h2p:{}", "a".repeat(MAX_PATCH_TAG_LEN + 1));
    assert!(matches!(
        parse_patch_wire(&long),
        Err(PatchWireError::InvalidTag { .. })
    ));
    assert!(matches!(
        parse_patch_wire("x.h2p:-"),
        Err(PatchWireError::InvalidTag { .. })
    ));
}

#[test]
fn unknown_extension_is_an_error_not_surge() {
    assert!(matches!(
        parse_patch_wire("x.unknown:"),
        Err(PatchWireError::UnknownExtension { .. })
    ));
    assert_eq!(patch_form_of_path("Pads/Pad 1"), None);
}

#[test]
fn empty_tag_routes_to_the_default_owner() {
    assert_eq!(owner("x.h2p:"), "TyrellN6");
    assert_eq!(owner("x.H2P:"), "TyrellN6");
    assert_eq!(owner("x.fxp:"), "Surge XT");
    let wire = parse_patch_wire(r"D:\d\x.fxp:").unwrap();
    assert_eq!(wire.path, r"D:\d\x.fxp");
    assert_eq!((wire.tag, wire.form), ("", PatchForm::StateFile));
}

#[test]
fn every_form_parses_with_an_empty_tag() {
    let cases = [
        ("Pads/Pad 1.fxp:", PatchForm::StateFile),
        ("Dexed/rom1a.syx/00 BRASS 1:", PatchForm::Cartridge),
        ("Presets/AB Pad.vvp:", PatchForm::Vvp),
        ("Lib/Piano.floe-preset:", PatchForm::FloePreset),
        ("Garritan/Glockenspiel.sfz:", PatchForm::Sfz),
        ("Keys/Airy Bells.ariax:", PatchForm::Sfz),
        ("Bass/Bass 1.sxsnp:", PatchForm::SixSines),
        ("01 Basses/Abgrund.h2p:", PatchForm::H2p),
        ("factory/dice_acid.json:", PatchForm::Jc303Preset),
        (r"slot=1;D:\c\track2_meas2.wav:", PatchForm::CacheWav),
        ("slot=1;:", PatchForm::CacheWav),
    ];
    for (wire, form) in cases {
        assert_eq!(parse_patch_wire(wire).unwrap().form, form, "{wire}");
    }
}

/// program 名に `:` を含む cartridge も、末尾の `:` で切れる。
#[test]
fn cartridge_program_name_may_contain_a_colon() {
    let wire = parse_patch_wire("Dexed/rom1a.syx/05 A:B:").unwrap();
    assert_eq!(wire.path, "Dexed/rom1a.syx/05 A:B");
    assert_eq!(wire.form, PatchForm::Cartridge);
}

#[test]
fn display_drops_only_the_empty_tag_and_wire_restores_it() {
    let wire = parse_patch_wire("01 Basses/Abgrund.h2p:").unwrap();
    assert_eq!(display_from_wire(&wire), "01 Basses/Abgrund.h2p");
    assert_eq!(
        wire_from_display("01 Basses/Abgrund.h2p"),
        "01 Basses/Abgrund.h2p:"
    );
    assert_eq!(wire_from_display("slot=1;"), "slot=1;:");
    assert_eq!(
        wire_from_display("Dexed/rom1a.syx/05 A:B"),
        "Dexed/rom1a.syx/05 A:B:"
    );
    for display in ["01 Basses/Abgrund.h2p", "Pads/Pad 1.fxp", r"D:\d\x.fxp"] {
        let wire = wire_from_display(display);
        assert_eq!(
            display_from_wire(&parse_patch_wire(&wire).unwrap()),
            display
        );
    }
}

#[test]
fn patch_ref_wire_goes_through_the_table() {
    let patch = crate::PatchRef {
        plugin: crate::PluginKey::from_identity(Some("com.u-he.TyrellN6"), "TyrellN6.clap"),
        display: "01 Basses/Abgrund.h2p".to_string(),
    };
    assert_eq!(patch.wire(), "01 Basses/Abgrund.h2p:");
}

/// client が音色一覧と突き合わせる表示名は、表に無い拡張子でも `:` さえあれば取り出せる。
#[test]
fn display_from_wire_str_needs_only_the_separator() {
    assert_eq!(
        display_from_wire_str("Pads/Pad 1.fxp:").as_deref(),
        Some("Pads/Pad 1.fxp")
    );
    assert_eq!(display_from_wire_str("piano:").as_deref(), Some("piano"));
    assert_eq!(
        display_from_wire_str("Dexed/rom1a.syx/05 A:B:").as_deref(),
        Some("Dexed/rom1a.syx/05 A:B")
    );
    assert_eq!(
        display_from_wire_str(r"D:\d\x.fxp:").as_deref(),
        Some(r"D:\d\x.fxp")
    );
    assert_eq!(display_from_wire_str("x.h2p:z").as_deref(), Some("x.h2p:z"));
    assert_eq!(display_from_wire_str("Pads/Pad 1.fxp"), None);
    assert_eq!(display_from_wire_str(r"D:\d\x.fxp"), None);
    for display in [
        "piano",
        "Pads/Pad 1.fxp",
        "Dexed/rom1a.syx/05 A:B",
        r"D:\d\x.fxp",
    ] {
        assert_eq!(
            display_from_wire_str(&wire_from_display(display)).as_deref(),
            Some(display)
        );
    }
}

#[test]
fn patch_display_from_embedded_json_ignores_legacy_keys_and_untagged_values() {
    let display =
        |json: &str| patch_display_from_embedded_json(&serde_json::from_str(json).unwrap());
    assert_eq!(
        display(r#"{"patch": "Pads/Pad 1.fxp:"}"#).as_deref(),
        Some("Pads/Pad 1.fxp")
    );
    assert_eq!(display(r#"{"patch": "Pads/Pad 1.fxp"}"#), None);
    assert_eq!(display(r#"{"Surge XT patch": "Pads/Pad 1.fxp:"}"#), None);
    assert_eq!(
        display(r#"{"patch": "x.fxp:", "Surge XT patch filter": "pad"}"#),
        None
    );
    assert_eq!(display(r#"{"beat": "4/4"}"#), None);
}

#[test]
fn the_z_tag_routes_h2p_to_zebralette3_only() {
    let wire = parse_patch_wire("1 Poly Keys/Applicant Replicant.h2p:z").unwrap();
    assert_eq!(wire.path, "1 Poly Keys/Applicant Replicant.h2p");
    assert_eq!(
        (wire.form, wire.tag, wire.owner),
        (PatchForm::H2p, "z", "Zebralette3")
    );
    assert!(matches!(
        parse_patch_wire("Pads/Pad 1.fxp:z"),
        Err(PatchWireError::UnknownTag { .. })
    ));
    let err = parse_patch_wire("x.h2p:y").unwrap_err();
    assert!(
        err.to_string().contains("TyrellN6 は ':'")
            && err.to_string().contains("Zebralette3 は ':z'"),
        "{err}"
    );
}

/// 表示名は非空タグを残すので、wire と表示名が同じ綴りになる。
#[test]
fn a_zebralette3_display_name_is_its_own_wire() {
    assert_eq!(wire_from_display("x.h2p:z"), "x.h2p:z");
    assert_eq!(display_from_wire_str("x.h2p:z").as_deref(), Some("x.h2p:z"));
    assert_eq!(
        display_from_wire(&parse_patch_wire("x.h2p:z").unwrap()),
        "x.h2p:z"
    );
    assert_eq!(
        display_from_wire_str(&wire_from_display("1 Poly Keys/x.h2p:z")).as_deref(),
        Some("1 Poly Keys/x.h2p:z")
    );
}

/// plugin 側のタグ（`patch_tag_of`）と表の追加タグが食い違うと、catalog に出た音色が鳴らない。
#[test]
fn every_u_he_product_tag_is_in_the_table() {
    for product in cmrt_server_config::U_HE_PRODUCTS {
        let found = rows()
            .filter(|r| r.form == PatchForm::H2p && r.tag == product.patch_tag)
            .map(|r| r.owner)
            .collect::<Vec<_>>();
        assert_eq!(found, [product.product], "{product:?}");
    }
}
