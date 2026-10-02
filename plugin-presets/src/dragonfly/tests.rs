use super::*;

/// DPF が保存する形の state（`preset` state と parameter 2 つ）。
fn dpf_state(preset: &str, params: &[(&str, &str)]) -> Vec<u8> {
    let mut tokens = vec![
        STATE_BEGIN,
        PRESET_STATE_KEY,
        preset,
        STATE_END,
        PARAMETERS_BEGIN,
    ];
    for (symbol, value) in params {
        tokens.push(symbol);
        tokens.push(value);
    }
    tokens.push(PARAMETERS_END);
    encode_state(
        &tokens
            .iter()
            .map(|token| token.to_string())
            .collect::<Vec<_>>(),
    )
}

const TEST_PLUGIN: DragonflyPlugin = DragonflyPlugin {
    plugin_id: "test.dragonfly",
    name: "Test Dragonfly",
    file_stem: "TestDragonfly",
    has_preset_state: true,
    symbols: &["decay", "size"],
    presets: &[DragonflyPreset {
        name: "Big",
        values: &[3.5, 40.0],
    }],
};

#[test]
fn blob_overwrites_parameters_and_the_preset_state() {
    let init = dpf_state(
        "Small",
        &[("dry_level", "80"), ("decay", "1.3"), ("size", "24")],
    );
    let blob = dragonfly_state_blob(&init, &TEST_PLUGIN, &TEST_PLUGIN.presets[0]).unwrap();
    assert_eq!(
        blob,
        dpf_state(
            "Big",
            &[("dry_level", "80"), ("decay", "3.5"), ("size", "40")]
        )
    );
}

#[test]
fn state_round_trips_through_parse_and_encode() {
    let init = dpf_state("Small", &[("decay", "1.3")]);
    assert_eq!(encode_state(&parse_state(&init).unwrap()), init);
    assert_eq!(*init.last().unwrap(), 0);
    assert_eq!(init[init.len() - 2], TERMINATOR);
}

#[test]
fn a_symbol_missing_from_the_template_is_an_error() {
    let init = dpf_state("Small", &[("decay", "1.3")]);
    let error = dragonfly_state_blob(&init, &TEST_PLUGIN, &TEST_PLUGIN.presets[0]).unwrap_err();
    assert!(error.to_string().contains("'size'"), "{error:#}");
}

#[test]
fn a_template_without_the_preset_state_is_an_error_when_expected() {
    let tokens = [PARAMETERS_BEGIN, "decay", "1", "size", "2", PARAMETERS_END];
    let init = encode_state(&tokens.iter().map(|t| t.to_string()).collect::<Vec<_>>());
    assert!(dragonfly_state_blob(&init, &TEST_PLUGIN, &TEST_PLUGIN.presets[0]).is_err());
}

#[test]
fn unknown_preset_name_is_an_error() {
    let error = TEST_PLUGIN.preset("Nope").unwrap_err();
    assert!(error.to_string().contains("'Nope'"), "{error:#}");
}

#[test]
fn every_table_row_has_one_value_per_symbol_and_a_unique_name() {
    for plugin in &DRAGONFLY_PLUGINS {
        assert!(!plugin.presets.is_empty(), "{}", plugin.name);
        let mut names: Vec<&str> = plugin.presets.iter().map(|preset| preset.name).collect();
        for preset in plugin.presets {
            assert_eq!(
                preset.values.len(),
                plugin.symbols.len(),
                "{}: {}",
                plugin.name,
                preset.name
            );
        }
        names.sort();
        names.dedup();
        assert_eq!(names.len(), plugin.presets.len(), "{}", plugin.name);
    }
}

#[test]
fn plugin_lookup_is_by_clap_id() {
    assert_eq!(
        dragonfly_plugin("michaelwillis.dragonfly.plate").map(|plugin| plugin.name),
        Some("Dragonfly Plate Reverb")
    );
    assert!(dragonfly_plugin("com.tone3000.plugin").is_none());
}
