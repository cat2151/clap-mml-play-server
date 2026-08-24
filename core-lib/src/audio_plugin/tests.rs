use super::*;
use cmrt_server_config::{DEXED_PLUGIN_ID, VAPORIZER2_PLUGIN_ID};

fn plugin(name: &str, id: &str) -> AudioPluginInfo {
    AudioPluginInfo::new(name, format!("{name}.clap"), Some(id.to_string()), None)
}

#[test]
fn routing_returns_plugin_keys_without_exposing_concrete_forms() {
    let catalog = AudioPluginCatalog::new(vec![
        plugin("Surge XT", SURGE_XT_PLUGIN_ID),
        plugin("Dexed", DEXED_PLUGIN_ID),
        plugin("Vaporizer2", VAPORIZER2_PLUGIN_ID),
    ]);
    assert_eq!(
        catalog.route_patch("Bank.syx/00 Init").unwrap().name,
        "Dexed"
    );
    assert_eq!(
        catalog.route_patch("PD Wide Pad.vvp").unwrap().name,
        "Vaporizer2"
    );
}

#[test]
fn duplicate_forms_are_an_explicit_error() {
    let catalog = AudioPluginCatalog::new(vec![
        plugin("First", "example.first"),
        plugin("Second", "example.second"),
    ]);
    assert!(matches!(
        catalog.route_patch("Pads/Shared.fxp"),
        Err(RouteError::Ambiguous { .. })
    ));
}

#[test]
fn concrete_layout_is_returned_as_neutral_sort_metadata() {
    let factory = patch_sort_metadata("patches_factory/Pads/Warm.fxp");
    assert_eq!(factory.category, "Pads");
    assert_eq!(factory.source_rank, 0);

    let third_party = patch_sort_metadata("patches_3rdparty/Vendor/Leads/Bright.fxp");
    assert_eq!(third_party.category, "Leads");
    assert_eq!(third_party.vendor, "Vendor");

    let vvp = patch_sort_metadata("PD Wide Pad.vvp");
    assert_eq!(vvp.category, "Pad");
}

#[test]
fn known_plugins_describe_selector_categories_without_client_branching() {
    let surge = plugin("Surge XT", SURGE_XT_PLUGIN_ID)
        .describe_patch("patches_factory/Basses/Attacky.fxp", None);
    assert_eq!(surge.selector_category.as_deref(), Some("Basses"));

    let vaporizer =
        plugin("Vaporizer2", VAPORIZER2_PLUGIN_ID).describe_patch("PD Wide Pad.vvp", None);
    assert_eq!(vaporizer.selector_category.as_deref(), Some("Pad"));

    let dexed = plugin("Dexed", DEXED_PLUGIN_ID).describe_patch("Factory.syx/00 Init", None);
    assert_eq!(dexed.selector_category, None);
}

#[test]
fn an_unknown_vaporizer_code_has_no_selector_category() {
    let patch =
        plugin("Vaporizer2", VAPORIZER2_PLUGIN_ID).describe_patch("ZZ Future Category.vvp", None);

    assert_eq!(patch.selector_category, None);
    assert_eq!(patch.sort.category, "ZZ");
}

#[test]
fn voicing_strategy_is_plugin_neutral_to_callers() {
    assert_eq!(
        plugin_voicing_source(Some(SURGE_XT_PLUGIN_ID), "ignored"),
        PluginVoicingSource::ExternalLookup
    );
    assert_eq!(
        plugin_voicing_source(Some(VAPORIZER2_PLUGIN_ID), "ignored"),
        PluginVoicingSource::CatalogMetadata
    );
    assert_eq!(
        plugin_voicing_source(Some(DEXED_PLUGIN_ID), "ignored"),
        PluginVoicingSource::AssumePoly
    );
}
