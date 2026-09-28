use super::*;

#[test]
fn recognizes_sfz_paths_case_insensitively() {
    assert!(is_sfz_patch_path("Garritan/Glockenspiel.sfz"));
    assert!(is_sfz_patch_path("Bank\\LOUD.SFZ"));
    assert!(!is_sfz_patch_path(".sfz"));
    assert!(!is_sfz_patch_path("notes.txt"));
}

#[test]
fn recognizes_ariax_paths_case_insensitively_as_sforzando_patches() {
    assert!(is_ariax_patch_path(
        "TableWarp2/Presets/Keys/Airy Bells.ariax"
    ));
    assert!(is_ariax_patch_path("Keys\\AIRY BELLS.ARIAX"));
    assert!(!is_ariax_patch_path(".ariax"));
    assert!(!is_ariax_patch_path("Garritan/Glockenspiel.sfz"));
    assert!(!is_sfz_patch_path("Keys/Airy Bells.ariax"));
    assert!(is_sforzando_patch_path("Keys/Airy Bells.ARIAX"));
    assert!(is_sforzando_patch_path("Garritan/Glockenspiel.sfz"));
    assert!(!is_sforzando_patch_path("Harp/Realistic.floe-preset"));
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

    let state =
        sforzando_state_blob(&init, &program(special), SfzStreaming::PluginDefault).unwrap();
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
    assert!(
        sforzando_state_blob(&invalid, &program("Piano"), SfzStreaming::PluginDefault)
            .unwrap_err()
            .to_string()
            .contains("XML")
    );

    let wrong_root = codec::encode(b"<Other><Slot/></Other>").unwrap();
    assert!(
        sforzando_state_blob(&wrong_root, &program("Piano"), SfzStreaming::PluginDefault)
            .unwrap_err()
            .to_string()
            .contains("AriaSave")
    );
}

#[test]
fn builder_inserts_minimal_slot_when_the_instance_template_has_none() {
    let without_slot = codec::encode(
        b"<AriaSave version=\"1982\"><Settings custom=\"kept\"/><EffectSlot/></AriaSave>",
    )
    .unwrap();

    let state = sforzando_state_blob(
        &without_slot,
        &program("Piano"),
        SfzStreaming::PluginDefault,
    )
    .unwrap();
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

fn settings_after(init_xml: &[u8], streaming: SfzStreaming) -> anyhow::Result<Element> {
    let init = codec::encode(init_xml).unwrap();
    let state = sforzando_state_blob(&init, &program("Piano"), streaming)?;
    let xml = codec::decode(&state).unwrap();
    let root = Element::parse(std::io::Cursor::new(xml.as_bytes())).unwrap();
    Ok(root.get_child("Settings").unwrap().clone())
}

const TEMPLATE_WITH_STREAMING: &[u8] =
    b"<AriaSave><Settings quality=\"1\" streaming=\"32\" maxStreamAllocMB=\"2048\"/><Slot id=\"0\"/></AriaSave>";

#[test]
fn disabled_streaming_sets_zero_and_keeps_other_settings() {
    let settings = settings_after(TEMPLATE_WITH_STREAMING, SfzStreaming::Disabled).unwrap();

    assert_eq!(settings.attributes["streaming"], "0");
    assert_eq!(settings.attributes["quality"], "1");
    assert_eq!(settings.attributes["maxStreamAllocMB"], "2048");
}

#[test]
fn plugin_default_streaming_keeps_the_template_value() {
    let settings = settings_after(TEMPLATE_WITH_STREAMING, SfzStreaming::PluginDefault).unwrap();

    assert_eq!(settings.attributes["streaming"], "32");
}

#[test]
fn disabled_streaming_without_settings_is_an_error() {
    let error = settings_after(
        b"<AriaSave><Slot id=\"0\"/></AriaSave>",
        SfzStreaming::Disabled,
    )
    .unwrap_err();

    assert!(error.to_string().contains("streaming"), "{error}");
}

const ARIAX: &str = r#"<?xml version="1.0" ?>
<AriaSave version="1844" productID="1014">
    <Settings quality="1" streaming="32" fromPreset="ignored" />
    <Slot id="0" name="TableWarp2" bankId="3103" version="1000" channel="-1" poly="32">
        <Main id="0" value="1" />
        <Param id="73" value="0.2599999904632568" />
        <Param id="111" value="0" />
    </Slot>
    <GUI id="0" selectedTab="1" />
</AriaSave>"#;

const TEMPLATE_WITH_GUI: &[u8] = br#"<AriaSave version="1982" productID="1014">
  <Settings quality="9" streaming="32" custom="preserve-me" />
  <Slot id="0" name="Init" bankId="0" version="0"><Main id="0" value="1" /></Slot>
  <EffectSlot id="0" name="Ambience" />
  <GUI id="0" selectedTab="0" />
</AriaSave>"#;

fn ariax_root(template: &[u8], ariax: &str, streaming: SfzStreaming) -> anyhow::Result<Element> {
    let init = codec::encode(template).unwrap();
    let state = sforzando_ariax_state_blob(&init, ariax, streaming)?;
    let xml = codec::decode(&state).unwrap();
    Ok(Element::parse(std::io::Cursor::new(xml.as_bytes())).unwrap())
}

fn param_values(slot: &Element) -> Vec<(String, String)> {
    slot.children
        .iter()
        .filter_map(|node| match node {
            XMLNode::Element(param) if param.name == "Param" => Some((
                param.attributes["id"].clone(),
                param.attributes["value"].clone(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn ariax_slot_replaces_the_template_slot_with_its_params() {
    let root = ariax_root(TEMPLATE_WITH_GUI, ARIAX, SfzStreaming::PluginDefault).unwrap();
    let slots: Vec<_> = root
        .children
        .iter()
        .filter(|node| matches!(node, XMLNode::Element(element) if element.name == "Slot"))
        .collect();
    let slot = root.get_child("Slot").unwrap();

    assert_eq!(slots.len(), 1);
    assert_eq!(slot.attributes["name"], "TableWarp2");
    assert_eq!(slot.attributes["bankId"], "3103");
    assert_eq!(slot.attributes["version"], "1000");
    assert_eq!(
        param_values(slot),
        vec![
            ("73".to_string(), "0.2599999904632568".to_string()),
            ("111".to_string(), "0".to_string()),
        ]
    );
}

#[test]
fn ariax_keeps_template_settings_effect_slot_and_gui() {
    let root = ariax_root(TEMPLATE_WITH_GUI, ARIAX, SfzStreaming::PluginDefault).unwrap();
    let settings = root.get_child("Settings").unwrap();

    assert_eq!(settings.attributes["quality"], "9");
    assert_eq!(settings.attributes["custom"], "preserve-me");
    assert_eq!(settings.attributes["streaming"], "32");
    assert!(!settings.attributes.contains_key("fromPreset"));
    assert_eq!(
        root.get_child("EffectSlot").unwrap().attributes["name"],
        "Ambience"
    );
    assert_eq!(
        root.get_child("GUI").unwrap().attributes["selectedTab"],
        "0"
    );
}

#[test]
fn ariax_with_disabled_streaming_zeroes_only_the_template_settings() {
    let root = ariax_root(TEMPLATE_WITH_GUI, ARIAX, SfzStreaming::Disabled).unwrap();
    let settings = root.get_child("Settings").unwrap();

    assert_eq!(settings.attributes["streaming"], "0");
    assert_eq!(settings.attributes["quality"], "9");
    assert!(!root
        .get_child("Slot")
        .unwrap()
        .attributes
        .contains_key("streaming"));
}

#[test]
fn ariax_slot_is_inserted_before_effect_slot_when_the_template_has_none() {
    let root = ariax_root(
        b"<AriaSave><Settings/><EffectSlot id=\"0\"/></AriaSave>",
        ARIAX,
        SfzStreaming::PluginDefault,
    )
    .unwrap();
    let names: Vec<_> = root
        .children
        .iter()
        .filter_map(|node| match node {
            XMLNode::Element(element) => Some(element.name.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(names, ["Settings", "Slot", "EffectSlot"]);
    assert_eq!(param_values(root.get_child("Slot").unwrap()).len(), 2);
}

#[test]
fn ariax_that_is_not_an_aria_save_is_an_error() {
    let wrong_root = ariax_root(
        TEMPLATE_WITH_GUI,
        "<AriaPreset><Slot id=\"0\"/></AriaPreset>",
        SfzStreaming::PluginDefault,
    )
    .unwrap_err();
    assert!(wrong_root.to_string().contains("AriaSave"), "{wrong_root}");

    let invalid =
        ariax_root(TEMPLATE_WITH_GUI, "<AriaSave>", SfzStreaming::PluginDefault).unwrap_err();
    assert!(invalid.to_string().contains("XML"), "{invalid}");

    let no_slot = ariax_root(
        TEMPLATE_WITH_GUI,
        "<AriaSave><Settings/></AriaSave>",
        SfzStreaming::PluginDefault,
    )
    .unwrap_err();
    assert!(no_slot.to_string().contains("Slot"), "{no_slot}");
}
