use super::*;

const INIT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?><APVTS><PARAM id="cutoff" value="0.5"/><PARAM id="modSlot3Amount" value="0.0"/><PARAM id="switchModState" value="0.0"/><PARAM id="filterType" value="0.0"/><PARAM id="volume" value="0.75"/><AcidSeq303 length="16"/></APVTS>"#;

fn param_values(state: &[u8]) -> Vec<(String, String)> {
    let root = Element::parse(Cursor::new(juce_xml::decode(state).unwrap().as_bytes())).unwrap();
    root.children
        .iter()
        .filter_map(|node| node.as_element())
        .filter(|element| element.name == "PARAM")
        .map(|element| {
            (
                element.attributes["id"].clone(),
                element.attributes["value"].clone(),
            )
        })
        .collect()
}

fn no_choices(id: &str, name: &str) -> Result<usize> {
    panic!("choice を引いてはいけない: {id}={name}")
}

/// 値は parameter 自身の単位のまま書く（-1..1 の amount を 0..1 へ正規化しない）。
#[test]
fn preset_values_are_written_in_the_parameters_own_units() {
    let json = r#"{"values":{"modSlot3Amount":-0.25,"switchModState":true,"filterType":"Spicy"}}"#;
    let state = jc303_state_blob(&juce_xml::encode(INIT_XML), json, |id, name| {
        assert_eq!((id, name), ("filterType", "Spicy"));
        Ok(1)
    })
    .unwrap();

    assert_eq!(
        param_values(&state),
        [
            ("cutoff", "0.5"),
            ("modSlot3Amount", "-0.25"),
            ("switchModState", "1"),
            ("filterType", "1"),
            ("volume", "0.75"),
        ]
        .map(|(id, value)| (id.to_string(), value.to_string()))
    );
}

/// preset に無い parameter と、PARAM 以外の子（sequencer の pattern）は init state のまま。
#[test]
fn unlisted_parameters_keep_the_init_values() {
    let state =
        jc303_state_blob(&juce_xml::encode(INIT_XML), r#"{"values":{}}"#, no_choices).unwrap();
    let xml = juce_xml::decode(&state).unwrap();

    assert_eq!(param_values(&state)[0], ("cutoff".into(), "0.5".into()));
    assert!(xml.contains(r#"<AcidSeq303 length="16" />"#), "{xml}");
}

#[test]
fn mod_knobs_are_written_and_default_like_the_plugin() {
    let decode_root = |state: Vec<u8>| {
        Element::parse(Cursor::new(juce_xml::decode(&state).unwrap().as_bytes())).unwrap()
    };
    let init = juce_xml::encode(INIT_XML);

    let given = decode_root(
        jc303_state_blob(
            &init,
            r#"{"modKnob1":"modSlot3Amount","modKnob2":"modSlot4Amount"}"#,
            no_choices,
        )
        .unwrap(),
    );
    assert_eq!(given.attributes["modKnob1"], "modSlot3Amount");
    assert_eq!(given.attributes["modKnob2"], "modSlot4Amount");

    let defaulted = decode_root(jc303_state_blob(&init, "{}", no_choices).unwrap());
    assert_eq!(defaulted.attributes["modKnob1"], "filterMorph");
    assert_eq!(defaulted.attributes["modKnob2"], "filterFm");
}

#[test]
fn a_parameter_missing_from_the_state_is_an_error() {
    let error = jc303_state_blob(
        &juce_xml::encode(INIT_XML),
        r#"{"values":{"newKnob":0.5}}"#,
        no_choices,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("newKnob"), "{error:#}");
}

#[test]
fn an_unknown_choice_is_an_error() {
    let result = jc303_state_blob(
        &juce_xml::encode(INIT_XML),
        r#"{"values":{"filterType":"Nope"}}"#,
        |_, name| anyhow::bail!("選択肢に無い: {name}"),
    );
    assert!(result.is_err());
}

#[test]
fn only_json_files_are_presets() {
    assert!(is_jc303_preset_path(r"factory\dice_acid.json"));
    assert!(is_jc303_preset_path("Dice Acid.JSON"));
    assert!(!is_jc303_preset_path("dice_acid.json.tmp"));
    assert!(!is_jc303_preset_path(".json"));
}
