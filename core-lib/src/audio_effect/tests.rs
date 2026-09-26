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
    preset_with_kind(plugin, value, path, "Test Category", "Test Kind")
}

fn preset_with_kind(
    plugin: &AudioEffectPluginInfo,
    value: &str,
    path: &str,
    category: &str,
    kind: &str,
) -> AudioEffectPreset {
    AudioEffectPreset {
        plugin: plugin.key.clone(),
        json_key: plugin.json_key.clone(),
        value: value.to_string(),
        display: format!("{}: {value}", plugin.name),
        name: value.to_string(),
        category: category.to_string(),
        kind: kind.to_string(),
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
    assert_eq!(listed.category, "Space / Imaging");
    assert_eq!(listed.kind, "Reverb");
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
fn surge_classification_follows_the_folder_table() {
    assert_eq!(
        scan::surge_classification("Reverb 2/Cathedral.srgfx", "Surge XT Effects"),
        ("Space / Imaging".to_string(), "Reverb".to_string())
    );
    assert_eq!(
        scan::surge_classification("Airwindows/Filter/Air.srgfx", "Surge XT Effects"),
        ("Filter / EQ".to_string(), "Filter".to_string())
    );
    assert_eq!(
        scan::surge_classification("Combulator/Sparkle.srgfx", "Surge XT Effects"),
        ("Filter / EQ".to_string(), "Filter".to_string())
    );
    assert_eq!(
        scan::surge_classification("Conditioner/Limiter 1.srgfx", "Surge XT Effects"),
        ("Dynamics".to_string(), "Limiter / Clipper".to_string())
    );
    assert_eq!(
        scan::surge_classification("Reverb 1/Hall.srgfx", "Surge XT Effects"),
        ("Space / Imaging".to_string(), "Reverb".to_string())
    );
}

#[test]
fn unknown_folder_becomes_its_own_category_and_kind() {
    assert_eq!(
        scan::surge_classification("Foo/x.srgfx", "Surge XT Effects"),
        ("Foo".to_string(), "Foo".to_string())
    );
    assert_eq!(
        scan::surge_classification("Airwindows/New/x.srgfx", "Surge XT Effects"),
        ("Airwindows/New".to_string(), "Airwindows/New".to_string())
    );
    assert_eq!(
        scan::surge_classification("x.srgfx", "Surge XT Effects"),
        (
            "Surge XT Effects".to_string(),
            "Surge XT Effects".to_string()
        )
    );
}

#[test]
fn folder_table_has_no_duplicate_keys() {
    let mut folders: Vec<&str> = scan::SURGE_FOLDER_CLASSIFICATION
        .iter()
        .map(|(folder, _, _)| *folder)
        .collect();
    let original_len = folders.len();
    folders.sort();
    folders.dedup();
    assert_eq!(folders.len(), original_len);
}

#[test]
fn tone3000_is_an_amp_simulator_under_distortion() {
    assert_eq!(scan::TONE3000_CATEGORY, "Distortion / Saturation");
    assert_eq!(scan::TONE3000_KIND, "Amp Simulator");
}

#[test]
fn categories_and_kinds_are_deduplicated_and_sorted() {
    let surge = plugin("Surge XT Effects", SURGE_FX_PLUGIN_ID);
    let tone = plugin("TONE3000", TONE3000_PLUGIN_ID);
    let presets = vec![
        preset_with_kind(&surge, "b", "/p/b", "Space / Imaging", "Reverb"),
        preset_with_kind(&surge, "a", "/p/a", "Space / Imaging", "Reverb"),
        preset_with_kind(
            &tone,
            "c",
            "/p/c",
            "Distortion / Saturation",
            "Amp Simulator",
        ),
    ];
    let catalog = AudioEffectCatalog::with_entries(vec![tone, surge], presets);
    assert_eq!(
        catalog.categories(),
        vec![
            "Distortion / Saturation".to_string(),
            "Space / Imaging".to_string()
        ]
    );
    assert_eq!(
        catalog.kinds_in(None),
        vec!["Amp Simulator".to_string(), "Reverb".to_string()]
    );
    assert_eq!(
        catalog.kinds_in(Some("Space / Imaging")),
        vec!["Reverb".to_string()]
    );
}

/// 実 install の `fx_presets` フォルダが無い環境（Linux CI など）では何もせず通す。
#[test]
fn installed_surge_presets_all_hit_the_table() {
    let root = PathBuf::from(r"C:\ProgramData\Surge XT\fx_presets");
    if !root.is_dir() {
        return;
    }
    let mut files = Vec::new();
    collect_srgfx_files(&root, &mut files);
    let mut unmatched = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let Some((folder, _)) = relative.rsplit_once('/') else {
            continue;
        };
        let hit = scan::SURGE_FOLDER_CLASSIFICATION
            .iter()
            .any(|(entry_folder, _, _)| *entry_folder == folder);
        if !hit {
            unmatched.push(folder.to_string());
        }
    }
    assert!(unmatched.is_empty(), "表に無いフォルダ: {unmatched:?}");
}

/// 実 install が無い環境（Linux CI など）では何もせず通す。
/// TUI の add overlay の category / kind pane の項目数を実機カタログで裏づける。
#[test]
fn installed_surge_catalog_has_five_categories_and_eighteen_kinds() {
    let root = PathBuf::from(r"C:\ProgramData\Surge XT\fx_presets");
    if !root.is_dir() {
        return;
    }
    let catalog = AudioEffectCatalog::discover();
    assert_eq!(catalog.categories().len(), 5, "{:?}", catalog.categories());
    let kinds = catalog.kinds_in(None);
    assert_eq!(kinds.len(), 18, "{kinds:?}");
}

#[cfg(test)]
fn collect_srgfx_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_srgfx_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("srgfx") {
            out.push(path);
        }
    }
}

mod dragonfly;
