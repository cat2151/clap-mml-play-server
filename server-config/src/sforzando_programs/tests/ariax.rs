use super::TempRoot;
use super::*;

fn ariax(name: &str, bank_id: &str, version: &str) -> String {
    format!(
        r#"<?xml version="1.0" ?><AriaSave version="1844" productID="1014"><Settings streaming="32"/><Slot id="0" name="{name}" bankId="{bank_id}" version="{version}"><Param id="73" value="0.25"/></Slot><GUI id="0"/></AriaSave>"#
    )
}

/// bank.xml と `.sfz`、その下の `Presets/` に `.ariax` を置いた installed bank。
fn bank_with_preset(root: &TempRoot, preset: &str) -> PathBuf {
    write(
        &root.0.join("Synth.bank.xml"),
        manifest("3103", "1000", &[("Synth", "Programs/Synth.sfz")]).as_bytes(),
    );
    write(&root.0.join("Programs").join("Synth.sfz"), b"<region>");
    let path = root.0.join("Presets").join("Keys").join("Bells.ariax");
    write(&path, preset.as_bytes());
    path
}

#[test]
fn ariax_matching_a_manifest_program_resolves_to_that_program() {
    let root = TempRoot::new("ariax_match");
    let path = bank_with_preset(&root, &ariax("Synth", "3103", "1000"));

    let preset = resolve_sforzando_preset(&path).unwrap();

    assert_eq!(preset.ariax_path, std::fs::canonicalize(&path).unwrap());
    assert!(preset.xml.contains(r#"<Param id="73""#));
    assert_eq!(preset.program.bank_id, "3103");
    assert_eq!(preset.program.bank_version, "1000");
    assert_eq!(preset.program.program_name, "Synth");
    assert_eq!(
        preset.program.sfz_path,
        std::fs::canonicalize(root.0.join("Programs").join("Synth.sfz")).unwrap()
    );
}

#[test]
fn upper_case_extension_is_an_ariax() {
    let root = TempRoot::new("ariax_upper");
    let path = bank_with_preset(&root, &ariax("Synth", "3103", "1000"));
    let upper = path.with_file_name("Bells.ARIAX");
    std::fs::rename(&path, &upper).unwrap();

    assert!(resolve_sforzando_preset(&upper).is_ok());
}

#[test]
fn ariax_whose_slot_is_not_in_the_manifest_is_an_error() {
    for (label, preset) in [
        ("name", ariax("Other", "3103", "1000")),
        ("bank_id", ariax("Synth", "-1", "1000")),
        ("version", ariax("Synth", "3103", "2000")),
    ] {
        let root = TempRoot::new(&format!("ariax_mismatch_{label}"));
        let path = bank_with_preset(&root, &preset);

        let error = format!("{:#}", resolve_sforzando_preset(&path).unwrap_err());

        assert!(
            error.contains("一致する AriaProgram がない"),
            "{label}: {error}"
        );
        assert!(error.contains("Synth.bank.xml"), "{label}: {error}");
    }
}

#[test]
fn ariax_without_a_program_slot_or_aria_save_root_is_an_error() {
    for (label, preset, expected) in [
        (
            "root",
            r#"<Other><Slot id="0" name="Synth" bankId="3103" version="1000"/></Other>"#,
            "AriaSave ではない",
        ),
        (
            "slot",
            r#"<AriaSave><GUI id="0"/></AriaSave>"#,
            "Slot id=\"0\"",
        ),
        (
            "attr",
            r#"<AriaSave><Slot id="0" name="Synth" version="1000"/></AriaSave>"#,
            "Slot/@bankId",
        ),
        ("xml", "<AriaSave>", "XML が不正"),
    ] {
        let root = TempRoot::new(&format!("ariax_invalid_{label}"));
        let path = bank_with_preset(&root, preset);

        let error = format!("{:#}", resolve_sforzando_preset(&path).unwrap_err());

        assert!(error.contains(expected), "{label}: {error}");
    }
}

#[test]
fn sfz_is_not_accepted_as_an_ariax() {
    let root = TempRoot::new("ariax_sfz");
    bank_with_preset(&root, &ariax("Synth", "3103", "1000"));

    let error = resolve_sforzando_preset(&root.0.join("Programs").join("Synth.sfz")).unwrap_err();

    assert!(error.to_string().contains(".ariax file"), "{error}");
}

#[test]
fn catalog_lists_only_ariax_presets_whose_slot_matches_the_manifest() {
    let root = TempRoot::new("ariax_catalog");
    let matching = bank_with_preset(&root, &ariax("Synth", "3103", "1000"));
    let presets = matching.parent().unwrap();
    let upper = presets.join("Pad.ARIAX");
    write(&upper, ariax("Synth", "3103", "1000").as_bytes());
    write(
        &presets.join("Foreign.ariax"),
        ariax("Synth", "-1", "1000").as_bytes(),
    );
    write(
        &presets.join("NoVersion.ariax"),
        br#"<AriaSave><Slot id="0" name="Synth" bankId="3103"/></AriaSave>"#,
    );
    write(&presets.join("Broken.ariax"), b"<AriaSave>");

    let resolution = installed_only(vec![installed("Synth", &[root.0.join("Synth.bank.xml")])]);

    let mut expected = [root.0.join("Programs").join("Synth.sfz"), matching, upper]
        .map(|path| std::fs::canonicalize(path).unwrap())
        .to_vec();
    expected.sort_by_key(|path| canonical_key(path));
    assert_eq!(resolution.resolved_patches.unwrap(), expected);
    assert!(resolution.source_error.is_none());
    assert!(resolution.notices.is_empty(), "{:?}", resolution.notices);
}
