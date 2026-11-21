//! 実 OB-Xf を使う。デバイスや GUI を開かず、音色適用・切替・state 復元を検証する。
use super::*;
use cmrt_server_config::{OBXF_PATCH_TAG, OBXF_PLUGIN_ID};

fn plugin() -> String {
    plugin_path("CMRT_TEST_OBXF_CLAP")
}

fn obxf_wire(path: &str) -> String {
    format!("{path}:{OBXF_PATCH_TAG}")
}

fn patches() -> Vec<std::path::PathBuf> {
    let mut patches = Vec::new();
    for root in plugin_presets::obxf::obxf_preset_dirs() {
        if std::path::Path::new(&root).is_dir() {
            patches.extend(crate::collect_patches(&root).unwrap());
        }
    }
    assert!(patches.len() >= 2, "OB-Xf の実音色が必要");
    patches
}

fn renderer(patch: Option<&str>) -> RealtimeRenderer {
    let entry = load_entry(&plugin()).unwrap();
    RealtimeRenderer::new(
        &CoreConfig {
            patch_path: patch.map(obxf_wire),
            ..test_config_with_plugin_id(OBXF_PLUGIN_ID)
        },
        &entry,
    )
    .unwrap()
}

fn saved_root(renderer: &mut RealtimeRenderer) -> xmltree::Element {
    let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
    let xml = crate::surge_fx_preset::juce_xml::decode(&state).unwrap();
    xmltree::Element::parse(xml.as_bytes()).unwrap()
}

fn assert_loaded(renderer: &mut RealtimeRenderer, patch: &std::path::Path) {
    let fxp = std::fs::read(patch).unwrap();
    // 実 factory FXP の JUCE XML に記録された値を期待値にする。
    assert_eq!(&fxp[16..20], b"OBXf");
    let xml = crate::surge_fx_preset::juce_xml::decode(&fxp[60..]).unwrap();
    let expected = xmltree::Element::parse(xml.trim_end_matches('\0').as_bytes()).unwrap();
    let state = saved_root(renderer);
    assert_eq!(state.name, "OB-Xf");
    let program = state.get_child("program").unwrap();
    assert_eq!(
        program.attributes["programName"],
        expected.attributes["programName"]
    );
    for id in ["Volume", "Osc1Pitch", "Osc2Detune"] {
        let actual: f64 = program.attributes[id].parse().unwrap();
        let expected: f64 = expected.attributes[id].parse().unwrap();
        assert!(
            (actual - expected).abs() < 1e-6,
            "{id}: {actual} != {expected}"
        );
    }
}

#[test]
#[ignore = "実 OB-Xf CLAP と factory 音色が必要"]
fn initial_load_switch_and_catalog_state_restore() {
    let patches = patches();
    let a = patches[0].to_str().unwrap();
    let b = patches
        .iter()
        .find(|p| p.to_string_lossy().contains("Basses"))
        .unwrap();
    let mut instance = renderer(Some(a));
    assert_loaded(&mut instance, &patches[0]);
    instance
        .set_patch(Some(&obxf_wire(b.to_str().unwrap())))
        .unwrap();
    assert_loaded(&mut instance, b);
    let samples = render_live_note(&mut instance);
    assert!(peak(&samples) > 0.001, "音が出ていない");
    assert!(samples.iter().all(|s| s.is_finite()));

    let state = crate::patch_state_prepare::prepare_catalog_clap_patch_state(
        OBXF_PLUGIN_ID,
        std::path::Path::new(&plugin()),
        &patches[0],
    )
    .unwrap();
    let mut restored = renderer(None);
    load_plugin_state(restored.plugin_instance_mut(), &state).unwrap();
    assert_loaded(&mut restored, &patches[0]);
    restored
        .set_patch(Some(&obxf_wire(b.to_str().unwrap())))
        .unwrap();
    assert_loaded(&mut restored, b);
    assert!(restored.set_patch(Some(&wire(a))).is_err());
}

#[test]
#[ignore = "実 OB-Xf CLAP と factory 音色が必要"]
fn every_installed_factory_preset_loads() {
    let patches = patches();
    let profile = cmrt_server_config::merged_plugin_profiles(&Default::default())
        .remove("OB-Xf")
        .unwrap();
    let source = cmrt_server_config::resolve_patch_catalog(
        profile.plugin_id.as_deref(),
        &profile.plugin_path,
        profile.patches_dirs.as_deref(),
    );
    let plugin = crate::AudioPluginInfo::new(
        "OB-Xf",
        profile.plugin_path,
        profile.plugin_id,
        source.base.clone(),
    );
    let catalog = crate::AudioPluginCatalog::new(vec![plugin]);
    let mut instance = renderer(None);
    for patch in &patches {
        let display = format!("{}:{OBXF_PATCH_TAG}", source.base.display(patch));
        assert_eq!(catalog.route_patch(&display).unwrap().name, "OB-Xf");
        let audio = catalog
            .route_patch(&display)
            .unwrap()
            .describe_patch(&display, Some(patch));
        assert_eq!(
            audio.selector_category.as_deref(),
            patch.parent().unwrap().file_name().unwrap().to_str()
        );
        assert_eq!(
            std::path::Path::new(&source.base.resolve(crate::split_display(&display).0)),
            patch
        );
        instance
            .set_patch(Some(&obxf_wire(patch.to_str().unwrap())))
            .unwrap_or_else(|error| panic!("{}: {error:#}", patch.display()));
        assert_loaded(&mut instance, patch);
    }
    eprintln!("OB-Xf: {} 音色の読み込みを確認", patches.len());
}
