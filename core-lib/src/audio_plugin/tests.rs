use super::*;
use cmrt_server_config::{
    CACHE_PLAYER_PLUGIN_ID, DEXED_PLUGIN_ID, FLOE_PLUGIN_ID, SFORZANDO_PLUGIN_ID,
    SIX_SINES_PLUGIN_ID, TYRELLN6_PLUGIN_ID, VAPORIZER2_PLUGIN_ID,
};

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
fn only_dry_patch_forms_report_no_builtin_effects() {
    let cases = [
        (DEXED_PLUGIN_ID, PatchForm::Cartridge, false),
        (SFORZANDO_PLUGIN_ID, PatchForm::Sfz, false),
        (FLOE_PLUGIN_ID, PatchForm::FloePreset, false),
        (SIX_SINES_PLUGIN_ID, PatchForm::SixSines, false),
        (TYRELLN6_PLUGIN_ID, PatchForm::TyrellN6, false),
        (VAPORIZER2_PLUGIN_ID, PatchForm::Vvp, true),
        (SURGE_XT_PLUGIN_ID, PatchForm::StateFile, true),
        (CACHE_PLAYER_PLUGIN_ID, PatchForm::CacheWav, true),
    ];
    for (id, form, expected) in cases {
        let info = plugin("p", id);
        assert_eq!(info.patch_form, form, "{id}");
        assert_eq!(info.has_builtin_effects(), expected, "{id}");
    }
}

#[test]
fn an_unknown_plugin_is_assumed_to_have_builtin_effects() {
    let info = AudioPluginInfo::new(
        "Mystery",
        "Mystery.clap",
        Some("com.example.x".into()),
        None,
    );
    assert!(info.has_builtin_effects());
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
        plugin_voicing_source(Some(SIX_SINES_PLUGIN_ID), "ignored"),
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

#[test]
fn describe_patch_finds_the_file_under_a_per_root_base() {
    let root = std::env::temp_dir().join(format!("cmrt_test_per_root_vvp_{}", std::process::id()));
    let dir = root.join("Presets");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mono.vvp"), vvp_xml("Mono")).unwrap();
    let info = AudioPluginInfo::new(
        "Vaporizer2",
        "Vaporizer2.clap",
        Some(VAPORIZER2_PLUGIN_ID.to_string()),
        PatchBase::per_root(&[dir.to_string_lossy().into_owned()]),
    );
    let patch = info.describe_patch("Presets/mono.vvp", None);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(known_voicing(&patch), PatchVoicing::Mono);
}

#[test]
fn ariax_presets_are_read_by_sforzando_like_sfz() {
    assert_eq!(patch_form_of_path("Keys/Airy Bells.ariax"), PatchForm::Sfz);
    assert_eq!(patch_form_of_path("Keys\\Airy Bells.ARIAX"), PatchForm::Sfz);
    assert_eq!(
        patch_form_of_path("Garritan/Glockenspiel.sfz"),
        PatchForm::Sfz
    );
}

#[test]
fn six_sines_patches_route_to_six_sines_only() {
    let catalog = AudioPluginCatalog::new(vec![
        plugin("Surge XT", SURGE_XT_PLUGIN_ID),
        plugin("Six Sines", SIX_SINES_PLUGIN_ID),
    ]);
    assert_eq!(
        catalog.route_patch("Bass/Bass 1.sxsnp").unwrap().name,
        "Six Sines"
    );
    assert_eq!(
        catalog
            .route_patch("patches_factory/Keys/EP.fxp")
            .unwrap()
            .name,
        "Surge XT"
    );
}

/// `.h2p` は TyrellN6 だけへ、`.fxp` は TyrellN6 へは行かない。mono/poly は読まない。
#[test]
fn tyrelln6_patches_route_to_tyrelln6_only() {
    let catalog = AudioPluginCatalog::new(vec![
        plugin("Surge XT", SURGE_XT_PLUGIN_ID),
        plugin("TyrellN6", TYRELLN6_PLUGIN_ID),
    ]);
    assert_eq!(
        catalog.route_patch("01 Basses/Abgrund.h2p").unwrap().name,
        "TyrellN6"
    );
    assert_eq!(
        catalog
            .route_patch("patches_factory/Keys/EP.fxp")
            .unwrap()
            .name,
        "Surge XT"
    );
    assert_eq!(
        patch_form_of_path("01 Basses\\Abgrund.H2P"),
        PatchForm::TyrellN6
    );
    assert_eq!(
        plugin_voicing_source(Some(TYRELLN6_PLUGIN_ID), "TyrellN6.clap"),
        PluginVoicingSource::AssumePoly
    );
}

/// voicing は再生モード（param 523）から読む。読めないときは Poly にせず Unknown。
#[test]
fn six_sines_voicing_comes_from_the_play_mode_param() {
    let dir = std::env::temp_dir().join(format!("cmrt_test_six_sines_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("Bass")).unwrap();
    let xml = |mode: &str| {
        format!(
            r#"<patch id="org.baconpaul.six-sines" version="6" name="X"><params><p id="500" v="0.75" /><p id="523" v="{mode}" /></params></patch>"#
        )
    };
    std::fs::write(dir.join("Bass").join("mono.sxsnp"), xml("1.000000")).unwrap();
    std::fs::write(dir.join("Bass").join("poly.sxsnp"), xml("0.000000")).unwrap();
    let info = AudioPluginInfo::new(
        "Six Sines",
        "Six Sines.clap",
        Some(SIX_SINES_PLUGIN_ID.to_string()),
        Some(dir.to_string_lossy().into_owned()),
    );
    let mono = info.describe_patch("Bass/mono.sxsnp", None);
    let poly = info.describe_patch("Bass/poly.sxsnp", None);
    let missing = info.describe_patch("Bass/missing.sxsnp", None);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(known_voicing(&mono), PatchVoicing::Mono);
    assert_eq!(known_voicing(&poly), PatchVoicing::Poly);
    assert_eq!(known_voicing(&missing), PatchVoicing::Unknown);
    assert_eq!(mono.sort.category, "Bass");
}
