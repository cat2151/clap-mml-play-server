//! Dragonfly Reverb の組み込み preset が catalog と chain に載ること。

use super::*;
use crate::dragonfly_preset::DRAGONFLY_PLUGINS;

const HALL_JSON_KEY: &str = "Dragonfly Hall Reverb preset";

/// Hall の plugin 本体（中身は空）を temp に置いて走査する。
fn scanned_hall() -> AudioEffectCatalog {
    let root = std::env::temp_dir().join(format!(
        "cmrt_dragonfly_catalog_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let plugin_path = root.join("DragonflyHallReverb.clap");
    std::fs::write(&plugin_path, b"").unwrap();
    let hall = &DRAGONFLY_PLUGINS[0];
    let catalog = AudioEffectCatalog::scan(vec![AudioEffectPluginInfo::new(
        hall.name,
        plugin_path.to_string_lossy().into_owned(),
        hall.plugin_id,
        &plugin_path,
    )]);
    std::fs::remove_dir_all(&root).unwrap();
    catalog
}

#[test]
fn builtin_presets_are_listed_without_preset_files() {
    let catalog = scanned_hall();
    assert_eq!(catalog.presets().len(), 25);
    assert!(catalog.skipped().is_empty(), "{:?}", catalog.skipped());
    let great_hall = catalog.find(HALL_JSON_KEY, "Great Hall").unwrap();
    assert_eq!(great_hall.display, "Dragonfly Hall Reverb: Great Hall");
    assert_eq!(great_hall.name, "Great Hall");
    assert_eq!(great_hall.category, "Space / Imaging");
    assert_eq!(great_hall.kind, "Reverb");
    assert!(great_hall.path.ends_with("DragonflyHallReverb.clap"));
}

#[test]
fn chain_stage_carries_the_preset_name_to_the_loader() {
    let catalog = scanned_hall();
    let chain = effect_chain_spec_from_embedded_json(
        &serde_json::json!({ EFFECT_CHAIN_JSON_KEY: [{ HALL_JSON_KEY: "Great Hall" }] }),
        &catalog,
    )
    .unwrap();
    assert_eq!(chain.len(), 1);
    assert_eq!(chain.stages()[0].preset.value, "Great Hall");
    assert_eq!(
        chain.stages()[0].preset.display,
        "Dragonfly Hall Reverb: Great Hall"
    );
}

#[test]
fn builtin_plugins_include_all_four_dragonfly_plugins() {
    let ids: Vec<String> = builtin_effect_plugins()
        .into_iter()
        .map(|plugin| plugin.plugin_id)
        .collect();
    for dragonfly in &DRAGONFLY_PLUGINS {
        assert!(ids.iter().any(|id| id == dragonfly.plugin_id), "{ids:?}");
    }
}
