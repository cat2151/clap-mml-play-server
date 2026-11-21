//! 実 Apricot。GUI・音声デバイスを開かず catalog → 選択 → state 復元を通す。
use super::*;
use cmrt_server_config::APRICOT_PLUGIN_ID;

fn catalog() -> (crate::AudioPluginInfo, Vec<std::path::PathBuf>) {
    let path = plugin_path("CMRT_TEST_APRICOT_CLAP");
    let source = cmrt_server_config::resolve_patch_catalog(Some(APRICOT_PLUGIN_ID), &path, None);
    assert!(source.source_error.is_none(), "{source:?}");
    let info =
        crate::AudioPluginInfo::new("Apricot", path, Some(APRICOT_PLUGIN_ID.into()), source.base);
    (info, source.resolved_patches.unwrap())
}

fn renderer(info: &crate::AudioPluginInfo, patch: Option<&std::path::Path>) -> RealtimeRenderer {
    let entry = load_entry(&info.plugin_path).unwrap();
    RealtimeRenderer::new(
        &CoreConfig {
            patch_path: patch.map(|p| wire(p.to_str().unwrap())),
            ..test_config_with_plugin_id(APRICOT_PLUGIN_ID)
        },
        &entry,
    )
    .unwrap()
}

#[test]
#[ignore = "実 Apricot CLAP が必要"]
fn factory_load_switch_render_and_catalog_state_restore() {
    let (info, patches) = catalog();
    assert_eq!(patches.len(), 90);
    let a = patches
        .iter()
        .find(|p| p.to_string_lossy().contains("BA Super Square"))
        .unwrap();
    let b = patches
        .iter()
        .find(|p| p.to_string_lossy().contains("LD Typical Pulse"))
        .unwrap();
    let mut instance = renderer(&info, Some(a));
    let state_a = save_plugin_state(instance.plugin_instance_mut()).unwrap();
    instance
        .set_patch(Some(&wire(b.to_str().unwrap())))
        .unwrap();
    let state_b = save_plugin_state(instance.plugin_instance_mut()).unwrap();
    assert_ne!(state_a, state_b, "音色が切り替わっていない");
    let samples = render_live_note(&mut instance);
    assert!(peak(&samples) > 0.001, "無音");
    assert!(samples.iter().all(|s| s.is_finite()));
    let prepared = crate::patch_state_prepare::prepare_catalog_clap_patch_state(
        APRICOT_PLUGIN_ID,
        std::path::Path::new(&info.plugin_path),
        a,
    )
    .unwrap();
    let mut restored = renderer(&info, None);
    load_plugin_state(restored.plugin_instance_mut(), &prepared).unwrap();
    assert_eq!(
        save_plugin_state(restored.plugin_instance_mut()).unwrap(),
        state_a
    );
    restored
        .set_patch(Some(&wire(b.to_str().unwrap())))
        .unwrap();
    assert_eq!(
        save_plugin_state(restored.plugin_instance_mut()).unwrap(),
        state_b
    );
    assert!(restored.set_patch(Some("X:/wrong.fxp:")).is_err());
    assert!(restored
        .set_patch(Some(&wire("X:/missing.synthpatch")))
        .is_err());
    let missing =
        std::path::Path::new(&info.plugin_path).join("patches/Factory Basses/Missing.synthpatch");
    assert!(restored
        .set_patch(Some(&wire(missing.to_str().unwrap())))
        .is_err());

    // CLAP state と preset file は同形式ではない。誤った FILE 入力を成功扱いしない。
    let user = std::env::temp_dir().join(format!("cmrt-apricot-{}.synthpatch", std::process::id()));
    std::fs::write(&user, &state_a).unwrap();
    let user_result = restored.set_patch(Some(&wire(user.to_str().unwrap())));
    std::fs::remove_file(&user).unwrap();
    assert!(user_result.is_err());
}

#[test]
#[ignore = "実 Apricot CLAP が必要"]
fn every_factory_patch_routes_and_changes_state() {
    let (info, patches) = catalog();
    let mut instance = renderer(&info, None);
    let catalog = crate::AudioPluginCatalog::new(vec![info.clone()]);
    let mut states = std::collections::HashSet::new();
    for patch in &patches {
        let display = info.patch_base.display(patch);
        assert_eq!(catalog.route_patch(&display).unwrap().name, "Apricot");
        assert_eq!(
            std::path::Path::new(&info.patch_base.resolve(&display)),
            patch
        );
        assert!(info
            .describe_patch(&display, Some(patch))
            .selector_category
            .is_some());
        instance
            .set_patch(Some(&wire(patch.to_str().unwrap())))
            .unwrap();
        let state = save_plugin_state(instance.plugin_instance_mut()).unwrap();
        let expected_name = patch.file_stem().unwrap().to_str().unwrap().as_bytes();
        assert!(
            state
                .windows(expected_name.len())
                .any(|part| part == expected_name),
            "{} の名前が state に反映されていない",
            patch.display()
        );
        states.insert(state);
    }
    assert_eq!(states.len(), patches.len(), "選択が反映されない音色がある");
}
