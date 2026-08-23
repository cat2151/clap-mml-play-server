use super::*;

#[test]
fn recognizes_sfz_paths_case_insensitively() {
    assert!(is_sfz_patch_path("Garritan/Glockenspiel.sfz"));
    assert!(is_sfz_patch_path("Bank\\LOUD.SFZ"));
    assert!(!is_sfz_patch_path(".sfz"));
    assert!(!is_sfz_patch_path("notes.txt"));
}

fn template_xml() -> &'static str {
    r#"<?xml version="1.0" ?>
<AriaSave version="1982" productID="1014">
  <Settings quality="9" custom="preserve-me" />
  <Slot id="0" name="Init" bankId="0" version="0" channel="-1">
    <Main id="0" value="1" />
  </Slot>
  <EffectSlot id="0" name="Ambience" />
</AriaSave>"#
}

fn program(name: &str) -> SforzandoProgramRef {
    SforzandoProgramRef {
        sfz_path: "fixture.sfz".into(),
        bank_id: "3102".to_string(),
        bank_version: "1001".to_string(),
        program_name: name.to_string(),
        source: "fixture manifest".to_string(),
    }
}

#[test]
fn cegp_uses_uncompressed_little_endian_length_and_round_trips_zlib() {
    let blob = codec::encode(template_xml().as_bytes()).unwrap();

    assert_eq!(&blob[..4], b"CEGP");
    assert_eq!(
        u32::from_le_bytes(blob[4..8].try_into().unwrap()) as usize,
        template_xml().len()
    );
    assert_eq!(codec::decode(&blob).unwrap(), template_xml());
}

#[test]
fn codec_errors_identify_header_magic_length_and_zlib_failures() {
    assert!(codec::decode(b"CEG")
        .unwrap_err()
        .to_string()
        .contains("header"));

    let mut wrong_magic = codec::encode(b"<AriaSave/>").unwrap();
    wrong_magic[..4].copy_from_slice(b"NOPE");
    assert!(codec::decode(&wrong_magic)
        .unwrap_err()
        .to_string()
        .contains("CEGP"));

    let mut wrong_length = codec::encode(b"<AriaSave/>").unwrap();
    wrong_length[4..8].copy_from_slice(&999_u32.to_le_bytes());
    assert!(codec::decode(&wrong_length)
        .unwrap_err()
        .to_string()
        .contains("length"));

    let mut wrong_zlib = b"CEGP".to_vec();
    wrong_zlib.extend_from_slice(&10_u32.to_le_bytes());
    wrong_zlib.extend_from_slice(b"not zlib");
    assert!(codec::decode(&wrong_zlib)
        .unwrap_err()
        .to_string()
        .contains("zlib"));
}

#[test]
fn builder_preserves_unrelated_nodes_and_escapes_program_attributes() {
    let init = codec::encode(template_xml().as_bytes()).unwrap();
    let special = "A & B <bright> \"quoted\" 'apostrophe'";

    let state = sforzando_state_blob(&init, &program(special)).unwrap();
    let xml = codec::decode(&state).unwrap();
    let root = Element::parse(std::io::Cursor::new(xml.as_bytes())).unwrap();
    let settings = root.get_child("Settings").unwrap();
    let slot = root.get_child("Slot").unwrap();
    let effect = root.get_child("EffectSlot").unwrap();

    assert_eq!(
        settings.attributes.get("quality").map(String::as_str),
        Some("9")
    );
    assert_eq!(
        settings.attributes.get("custom").map(String::as_str),
        Some("preserve-me")
    );
    assert_eq!(
        slot.attributes.get("name").map(String::as_str),
        Some(special)
    );
    assert_eq!(
        slot.attributes.get("bankId").map(String::as_str),
        Some("3102")
    );
    assert_eq!(
        slot.attributes.get("version").map(String::as_str),
        Some("1001")
    );
    assert_eq!(
        slot.attributes.get("channel").map(String::as_str),
        Some("-1")
    );
    assert_eq!(
        effect.attributes.get("name").map(String::as_str),
        Some("Ambience")
    );
    assert!(xml.contains("&amp;"));
    assert!(xml.contains("&lt;"));
    assert!(xml.contains("&quot;"));
}

#[test]
fn builder_rejects_invalid_xml_and_wrong_root() {
    let invalid = codec::encode(b"<AriaSave>").unwrap();
    assert!(sforzando_state_blob(&invalid, &program("Piano"))
        .unwrap_err()
        .to_string()
        .contains("XML"));

    let wrong_root = codec::encode(b"<Other><Slot/></Other>").unwrap();
    assert!(sforzando_state_blob(&wrong_root, &program("Piano"))
        .unwrap_err()
        .to_string()
        .contains("AriaSave"));
}

#[test]
fn builder_inserts_minimal_slot_when_the_instance_template_has_none() {
    let without_slot = codec::encode(
        b"<AriaSave version=\"1982\"><Settings custom=\"kept\"/><EffectSlot/></AriaSave>",
    )
    .unwrap();

    let state = sforzando_state_blob(&without_slot, &program("Piano")).unwrap();
    let xml = codec::decode(&state).unwrap();
    let root = Element::parse(std::io::Cursor::new(xml.as_bytes())).unwrap();
    let slot = root.get_child("Slot").unwrap();

    assert_eq!(
        root.get_child("Settings").unwrap().attributes["custom"],
        "kept"
    );
    assert_eq!(slot.attributes["name"], "Piano");
    assert_eq!(slot.attributes["poly"], "32");
    assert!(slot.get_child("Main").is_some());
}
