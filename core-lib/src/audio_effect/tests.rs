use std::path::PathBuf;

use super::*;
use crate::surge_fx_preset::{SURGE_FX_PARAM_COUNT, SURGE_FX_PARAM_LAYOUTS};

const SURGE_FX_JSON_KEY: &str = "Surge XT Effects preset";
const TONE3000_JSON_KEY: &str = "TONE3000 preset";

fn plugin(name: &str, plugin_id: &str) -> AudioEffectPluginInfo {
    AudioEffectPluginInfo::new(
        name,
        format!("/plugins/{name}.clap"),
        plugin_id,
        format!("/presets/{name}"),
    )
}

fn preset(plugin: &AudioEffectPluginInfo, value: &str, path: &str) -> AudioEffectPreset {
    preset_with_role(plugin, value, path, "Test Role")
}

fn preset_with_role(
    plugin: &AudioEffectPluginInfo,
    value: &str,
    path: &str,
    role: &str,
) -> AudioEffectPreset {
    AudioEffectPreset {
        plugin: plugin.key.clone(),
        json_key: plugin.json_key.clone(),
        value: value.to_string(),
        display: format!("{}: {value}", plugin.name),
        name: value.to_string(),
        role: role.to_string(),
        path: PathBuf::from(path),
    }
}

/// TONE3000 1 件、Surge XT Effects 2 件（うち同名 2 件で曖昧になる値を 1 つ）。
fn catalog() -> AudioEffectCatalog {
    let surge = plugin("Surge XT Effects", SURGE_FX_PLUGIN_ID);
    let tone = plugin("TONE3000", TONE3000_PLUGIN_ID);
    let presets = vec![
        preset(
            &tone,
            "Bogner Fullstack",
            "/presets/TONE3000/Factory/a.t3kpreset",
        ),
        preset(&tone, "Twin", "/presets/TONE3000/Factory/b.t3kpreset"),
        preset(&tone, "Twin", "/presets/TONE3000/Factory/c.t3kpreset"),
        preset(
            &surge,
            "Reverb 1/Cathedral 2.srgfx",
            "/presets/Surge XT Effects/Reverb 1/Cathedral 2.srgfx",
        ),
    ];
    AudioEffectCatalog::with_entries(vec![tone, surge], presets)
}

fn spec(json: &str) -> Result<EffectChainSpec> {
    effect_chain_spec_from_embedded_json(&serde_json::from_str(json).unwrap(), &catalog())
}

#[test]
fn json_key_is_the_plugin_name_followed_by_preset() {
    assert_eq!(
        plugin("TONE3000", TONE3000_PLUGIN_ID).json_key,
        TONE3000_JSON_KEY
    );
    assert_eq!(
        plugin("Surge XT Effects", SURGE_FX_PLUGIN_ID).json_key,
        SURGE_FX_JSON_KEY
    );
}

#[test]
fn preset_json_element_is_a_single_key_object() {
    let tone = plugin("TONE3000", TONE3000_PLUGIN_ID);
    let element = preset(&tone, "Bogner Fullstack", "/x.t3kpreset").json_element();
    assert_eq!(
        element,
        serde_json::json!({ TONE3000_JSON_KEY: "Bogner Fullstack" })
    );
}

#[test]
fn missing_key_means_no_effects() {
    let chain = spec(r#"{"Surge XT patch": "Pads/Pad 1.fxp"}"#).unwrap();
    assert!(chain.is_empty());
    assert!(!embedded_json_has_effect_chain(Some(
        r#"{"Surge XT patch": "Pads/Pad 1.fxp"}"#
    )));
    assert!(!embedded_json_has_effect_chain(None));
}

#[test]
fn empty_array_means_no_effects() {
    let chain = spec(r#"{"effects after instrument": []}"#).unwrap();
    assert!(chain.is_empty());
    assert!(embedded_json_has_effect_chain(Some(
        r#"{"effects after instrument": []}"#
    )));
}

#[test]
fn chain_order_follows_the_array() {
    let chain = spec(
        r#"{"Surge XT patch": "Pads/Pad 1.fxp",
            "effects after instrument": [
              {"TONE3000 preset": "Bogner Fullstack"},
              {"Surge XT Effects preset": "Reverb 1/Cathedral 2.srgfx"}
            ]}"#,
    )
    .unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(
        chain.stages()[0].plugin,
        PluginKey::from_identity(Some(TONE3000_PLUGIN_ID), "")
    );
    assert_eq!(
        chain.stages()[0].preset.path,
        PathBuf::from("/presets/TONE3000/Factory/a.t3kpreset")
    );
    assert_eq!(
        chain.stages()[1].plugin,
        PluginKey::from_identity(Some(SURGE_FX_PLUGIN_ID), "")
    );
    assert_eq!(
        chain.to_string(),
        "[TONE3000: Bogner Fullstack -> Surge XT Effects: Reverb 1/Cathedral 2.srgfx]"
    );
}

#[test]
fn unknown_key_is_an_error() {
    let error = spec(r#"{"effects after instrument": [{"Reverb preset": "x"}]}"#).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("'Reverb preset'"), "{message}");
    assert!(message.contains("1 番目"), "{message}");
    assert!(message.contains(TONE3000_JSON_KEY), "{message}");
}

#[test]
fn unlisted_preset_is_an_error() {
    let error = spec(r#"{"effects after instrument": [{"TONE3000 preset": "Nope"}]}"#).unwrap_err();
    assert!(format!("{error:#}").contains("catalog に無い"), "{error:#}");
}

#[test]
fn ambiguous_preset_name_is_an_error() {
    let error = spec(r#"{"effects after instrument": [{"TONE3000 preset": "Twin"}]}"#).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("一意に決められない"), "{message}");
    assert!(message.contains("b.t3kpreset") && message.contains("c.t3kpreset"));
}

#[test]
fn element_shape_is_validated() {
    for json in [
        r#"{"effects after instrument": {"TONE3000 preset": "Twin"}}"#,
        r#"{"effects after instrument": ["Twin"]}"#,
        r#"{"effects after instrument": [{}]}"#,
        r#"{"effects after instrument": [{"TONE3000 preset": "a", "Surge XT Effects preset": "b"}]}"#,
        r#"{"effects after instrument": [{"TONE3000 preset": 1}]}"#,
    ] {
        assert!(spec(json).is_err(), "{json}");
    }
}

#[test]
fn scan_skips_plugins_whose_binary_is_missing() {
    let catalog = AudioEffectCatalog::scan(vec![plugin("TONE3000", TONE3000_PLUGIN_ID)]);
    assert!(catalog.plugins().is_empty());
    assert!(catalog.presets().is_empty());
}

#[test]
fn scan_lists_readable_srgfx_files_and_reports_the_rest() {
    let root = std::env::temp_dir().join(format!(
        "cmrt_effect_catalog_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let presets = root.join("fx_presets");
    std::fs::create_dir_all(presets.join("Reverb 1")).unwrap();
    let plugin_path = root.join("Surge XT Effects.clap");
    std::fs::write(&plugin_path, b"").unwrap();
    let params: String = (0..SURGE_FX_PARAM_COUNT)
        .map(|index| format!(r#" p{index}="0""#))
        .collect();
    std::fs::write(
        presets.join("Reverb 1").join("Cathedral 2.srgfx"),
        format!(
            r#"<single-fx streaming_version="17"><snapshot type="{}" name="Cathedral 2"{params}/></single-fx>"#,
            SURGE_FX_PARAM_LAYOUTS[0].fx_type
        ),
    )
    .unwrap();
    std::fs::write(presets.join("Broken.srgfx"), "<single-fx>").unwrap();
    std::fs::write(presets.join("notes.txt"), "ignored").unwrap();

    let catalog = AudioEffectCatalog::scan(vec![AudioEffectPluginInfo::new(
        "Surge XT Effects",
        plugin_path.to_string_lossy().into_owned(),
        SURGE_FX_PLUGIN_ID,
        &presets,
    )]);
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(catalog.plugins().len(), 1);
    assert_eq!(catalog.presets().len(), 1);
    let listed = &catalog.presets()[0];
    assert_eq!(listed.json_key, SURGE_FX_JSON_KEY);
    assert_eq!(listed.value, "Reverb 1/Cathedral 2.srgfx");
    assert_eq!(listed.display, "Surge XT Effects: Reverb 1/Cathedral 2");
    assert_eq!(listed.name, "Reverb 1/Cathedral 2");
    assert_eq!(listed.role, "Reverb 1");
    assert_eq!(catalog.skipped().len(), 1);
    assert!(catalog.skipped()[0].contains("Broken.srgfx"));
    assert!(catalog
        .find(SURGE_FX_JSON_KEY, "Reverb 1/Cathedral 2.srgfx")
        .is_ok());
}

#[test]
fn bypass_true_stage_is_dropped_from_the_chain() {
    let chain = spec(
        r#"{"effects after instrument": [
              {"TONE3000 preset": "Bogner Fullstack"},
              {"Surge XT Effects preset": "Reverb 1/Cathedral 2.srgfx", "bypass": true}
            ]}"#,
    )
    .unwrap();
    assert_eq!(chain.len(), 1);
    assert_eq!(
        chain.stages()[0].preset.path,
        PathBuf::from("/presets/TONE3000/Factory/a.t3kpreset")
    );
}

#[test]
fn bypass_false_stage_behaves_like_no_bypass_key() {
    let chain = spec(
        r#"{"effects after instrument": [
              {"TONE3000 preset": "Bogner Fullstack", "bypass": false}
            ]}"#,
    )
    .unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn bypass_must_be_a_bool() {
    let error = spec(
        r#"{"effects after instrument": [
              {"TONE3000 preset": "Bogner Fullstack", "bypass": "yes"}
            ]}"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("bool"), "{error:#}");
}

#[test]
fn two_plugin_keys_besides_bypass_is_still_an_error() {
    let error = spec(
        r#"{"effects after instrument": [
              {"TONE3000 preset": "a", "Surge XT Effects preset": "b", "bypass": true}
            ]}"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("キー 1 つ"), "{error:#}");
}

#[test]
fn surge_role_is_the_leading_segment_and_airwindows_becomes_multi_purpose() {
    assert_eq!(
        scan::surge_role_from_value("Reverb 2/Cathedral.srgfx", "Surge XT Effects"),
        "Reverb 2"
    );
    assert_eq!(
        scan::surge_role_from_value("Airwindows/Sub/Foo.srgfx", "Surge XT Effects"),
        "MultiPurpose"
    );
    assert_eq!(
        scan::surge_role_from_value("Foo.srgfx", "Surge XT Effects"),
        "Surge XT Effects"
    );
}

#[test]
fn tone3000_role_is_amp_simulator() {
    assert_eq!(scan::TONE3000_ROLE, "Amp Simulator");
}

#[test]
fn roles_are_deduplicated_and_sorted() {
    let surge = plugin("Surge XT Effects", SURGE_FX_PLUGIN_ID);
    let tone = plugin("TONE3000", TONE3000_PLUGIN_ID);
    let presets = vec![
        preset_with_role(&surge, "b", "/p/b", "Reverb 2"),
        preset_with_role(&surge, "a", "/p/a", "Reverb 2"),
        preset_with_role(&tone, "c", "/p/c", "Amp Simulator"),
    ];
    let catalog = AudioEffectCatalog::with_entries(vec![tone, surge], presets);
    assert_eq!(
        catalog.roles(),
        vec!["Amp Simulator".to_string(), "Reverb 2".to_string()]
    );
}
