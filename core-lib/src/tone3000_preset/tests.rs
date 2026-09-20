use super::*;

fn leaf(type_name: &str, properties: Vec<(&str, Var)>) -> ValueTree {
    ValueTree {
        type_name: type_name.to_string(),
        properties: properties
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
        children: Vec::new(),
    }
}

fn param(id: &str, value: f64) -> ValueTree {
    leaf(
        "Param",
        vec![
            ("id", Var::String(id.to_string())),
            ("value", Var::Double(value)),
        ],
    )
}

fn sample_preset() -> Vec<u8> {
    let mut chain = leaf("ChainSnapshot", vec![("v", Var::Int(1))]);
    chain.children.push(leaf(
        "ChainBlocks",
        vec![("type", Var::String("nam".to_string()))],
    ));
    let tree = ValueTree {
        type_name: "T3KPreset".to_string(),
        properties: vec![
            ("schemaVersion".to_string(), Var::Int(1)),
            ("name".to_string(), Var::String("Test Amp".to_string())),
        ],
        children: vec![
            chain,
            ValueTree {
                type_name: "Params".to_string(),
                properties: Vec::new(),
                children: vec![param("gain", 0.75), param("extra", 0.1)],
            },
        ],
    };
    let mut bytes = b"T3KB".to_vec();
    bytes.extend(encode(&tree));
    bytes
}

fn sample_init_state() -> Vec<u8> {
    let mut parameter = param("gain", 0.5);
    parameter.type_name = "PARAM".to_string();
    let tree = ValueTree {
        type_name: "TONE3000State".to_string(),
        properties: vec![
            ("schemaVersion".to_string(), Var::Int(1)),
            ("activePresetId".to_string(), Var::String(String::new())),
            ("activePresetName".to_string(), Var::String(String::new())),
        ],
        children: vec![
            ValueTree {
                type_name: "PARAMETERS".to_string(),
                properties: Vec::new(),
                children: vec![parameter],
            },
            leaf("MidiMappings", Vec::new()),
            leaf("ChainSnapshot", vec![("v", Var::Int(0))]),
        ],
    };
    let mut bytes = b"T3KB".to_vec();
    bytes.extend(encode(&tree));
    bytes
}

#[test]
fn parses_the_preset_name() {
    let preset = parse_t3k_preset(&sample_preset()).unwrap();
    assert_eq!(preset.name, "Test Amp");
    assert!(preset.tree.child("ChainSnapshot").is_some());
}

#[test]
fn rejects_wrong_magic_or_root() {
    assert!(parse_t3k_preset(b"VC2!xxxx").is_err());
    assert!(parse_t3k_preset(&sample_init_state()).is_err());
    assert!(parse_state(&sample_preset()).is_err());
}

#[test]
fn state_blob_replaces_chain_and_parameters_and_names_the_preset() {
    let preset = parse_t3k_preset(&sample_preset()).unwrap();
    let blob = tone3000_state_blob(&sample_init_state(), &preset, "uuid-1").unwrap();
    let state = parse_state(&blob).unwrap();
    assert_eq!(state.property_string("activePresetId"), Some("uuid-1"));
    assert_eq!(state.property_string("activePresetName"), Some("Test Amp"));
    // property の順は template のまま。
    assert_eq!(state.properties[1].0, "activePresetId");
    let chain = state.child("ChainSnapshot").unwrap();
    assert_eq!(chain.property("v"), Some(&Var::Int(1)));
    assert_eq!(chain.children[0].property_string("type"), Some("nam"));
    let parameters = state.child("PARAMETERS").unwrap();
    assert_eq!(parameters.children[0].type_name, "PARAM");
    assert_eq!(
        parameters.children[0].property("value"),
        Some(&Var::Double(0.75))
    );
    // template に無い parameter は末尾へ足す。
    assert_eq!(parameters.children[1].property_string("id"), Some("extra"));
    // template の子の順（PARAMETERS, MidiMappings, ChainSnapshot）は保つ。
    let order: Vec<&str> = state
        .children
        .iter()
        .map(|child| child.type_name.as_str())
        .collect();
    assert_eq!(order, ["PARAMETERS", "MidiMappings", "ChainSnapshot"]);
}
