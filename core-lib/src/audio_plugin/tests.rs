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
