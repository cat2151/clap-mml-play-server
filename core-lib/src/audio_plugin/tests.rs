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
fn a_lowercase_category_code_lands_in_the_same_category() {
    let vaporizer = plugin("Vaporizer2", VAPORIZER2_PLUGIN_ID);
    let lower = vaporizer.describe_patch("pd wide pad.vvp", None);
    let upper = vaporizer.describe_patch("PD Wide Pad.vvp", None);

    assert_eq!(lower.selector_category.as_deref(), Some("Pad"));
    assert_eq!(lower.selector_category, upper.selector_category);
    assert_eq!(lower.sort.category, "Pad");
    assert_eq!(lower.sort.category, upper.sort.category);
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

fn vvp_xml(poly_mode: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n\
         <VASTvaporizer2 PatchVersion=\"VASTVaporizerParamsV2.20000\" PatchName=\"Wide Pad\"\r\n\
         \x20               PatchCategory=\"PD\" PatchTag=\"Factory\" PatchAuthor=\"VASTDynamics\"\r\n\
         \x20               PatchComments=\"\">\r\n\
         \x20 <PARAM id=\"m_fMasterVolumedB\" text=\"0\"/>\r\n\
         \x20 <PARAM id=\"m_uPolyMode\" text=\"{poly_mode}\"/>\r\n\
         </VASTvaporizer2>\r\n"
    )
    .into_bytes()
}

fn describe_vvp_file(name: &str, bytes: &[u8]) -> AudioPatch {
    let dir = std::env::temp_dir().join("cmrt_test_unreadable_vvp");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let patch = plugin("Vaporizer2", VAPORIZER2_PLUGIN_ID).describe_patch(name, Some(&path));
    let _ = std::fs::remove_file(&path);
    patch
}

fn known_voicing(patch: &AudioPatch) -> PatchVoicing {
    match &patch.voicing {
        PatchVoicingHint::Known { voicing } => *voicing,
        other => panic!("Vaporizer2 の voicing は catalog metadata 由来のはず: {other:?}"),
    }
}

#[test]
fn an_unreadable_vvp_is_reported_as_unknown_not_poly() {
    let missing = std::env::temp_dir()
        .join("cmrt_test_unreadable_vvp")
        .join("does not exist.vvp");
    let _ = std::fs::remove_file(&missing);
    let missing = plugin("Vaporizer2", VAPORIZER2_PLUGIN_ID)
        .describe_patch("does not exist.vvp", Some(&missing));
    assert_eq!(known_voicing(&missing), PatchVoicing::Unknown);

    let not_vvp = describe_vvp_file("not a vvp.vvp", b"CcnK\0\0\0\0FPCh");
    assert_eq!(known_voicing(&not_vvp), PatchVoicing::Unknown);

    let mono = describe_vvp_file("mono.vvp", &vvp_xml("Mono"));
    assert_eq!(known_voicing(&mono), PatchVoicing::Mono);

    let poly = describe_vvp_file("poly.vvp", &vvp_xml("Poly16"));
    assert_eq!(known_voicing(&poly), PatchVoicing::Poly);
}
