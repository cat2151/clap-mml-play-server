//! 実 Floe CLAP と全 `.floe-preset` を使う統合テスト。

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::c_void;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

use clack_extensions::state::PluginState;

use super::*;
use crate::floe::FLOE_PLUGIN_ID;

const FLOE_CLAP_ENV: &str = "CMRT_TEST_FLOE_CLAP";
const FLOE_PRESETS_ENV: &str = "CMRT_TEST_FLOE_PRESETS";
const EXPECTED_PRESET_COUNT: usize = 13;
const AUDIBLE_PEAK: f32 = 1.0e-5;
const TEST_KEYS: [u8; 6] = [36, 48, 60, 72, 84, 96];

fn floe_presets() -> Vec<PathBuf> {
    let root = plugin_path(FLOE_PRESETS_ENV);
    let presets = crate::collect_patches(&root).unwrap();
    assert_eq!(
        presets.len(),
        EXPECTED_PRESET_COUNT,
        "Floe preset は {EXPECTED_PRESET_COUNT} 件でなければならない: {root}"
    );
    assert!(presets
        .iter()
        .all(|path| crate::is_floe_preset_path(&path.to_string_lossy())));
    presets
}

fn state_digest(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn render_key(renderer: &mut RealtimeRenderer, key: u8) -> Vec<f32> {
    renderer.reset();
    let mut samples = renderer.render_live_chunk(&[[0x90, key, 110]]).unwrap();
    for _ in 0..31 {
        samples.extend(renderer.render_live_chunk(&[]).unwrap());
    }
    samples.extend(renderer.render_live_chunk(&[[0x80, key, 0]]).unwrap());
    samples
}

fn assert_floe_capabilities(renderer: &mut RealtimeRenderer) {
    assert_eq!(renderer.plugin_id, FLOE_PLUGIN_ID);
    assert_eq!(renderer.capabilities.audio_output_ports, 1);
    assert_eq!(renderer.capabilities.main_output_channels, 2);
    assert!(renderer.capabilities.main_output_is_main);
    assert_eq!(renderer.capabilities.input_note_ports, 1);

    let handle = renderer.plugin_instance_mut().plugin_handle();
    assert!(handle.get_extension::<PluginState>().is_some());
    let get_extension = handle.as_raw().get_extension.unwrap();
    // SAFETY: Floe instance は生存中。extension ID の問い合わせは CLAP の契約内。
    let custom: *const c_void =
        unsafe { get_extension(handle.as_raw_ptr(), c"floe.floe".as_ptr()) };
    assert!(!custom.is_null());
}

/// Floe は同一 DLL を deinit 後に同じプロセスで init し直せないため、実機観点を 1 test・
/// 1 PluginEntry にまとめる。通常の `cargo test -- --ignored` でも lifecycle に依存しない。
#[test]
#[ignore = "実 Floe CLAP と全 preset が要る"]
fn floe_real_plugin_and_all_presets_satisfy_the_render_contract() {
    let presets = floe_presets();
    let entry = load_entry(&plugin_path(FLOE_CLAP_ENV)).unwrap();

    {
        let mut renderer =
            RealtimeRenderer::new(&test_config_with_plugin_id(FLOE_PLUGIN_ID), &entry).unwrap();
        assert_floe_capabilities(&mut renderer);

        let initial = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        let initial_digest = state_digest(&initial);
        let mut saved_digests = BTreeMap::new();

        for preset in &presets {
            let display = preset.to_string_lossy().into_owned();
            renderer.set_patch(Some(&display)).unwrap();
            let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
            let digest = state_digest(&state);
            assert_ne!(digest, initial_digest, "初期 state のまま: {display}");

            let mut best_peak = 0.0_f32;
            for key in TEST_KEYS {
                best_peak = best_peak.max(peak(&render_key(&mut renderer, key)));
            }
            assert!(
                best_peak > AUDIBLE_PEAK,
                "全 key で無音: {display} peak={best_peak}"
            );
            eprintln!(
                "floe preset='{}' peak={best_peak:.6} state_digest={digest:016x}",
                display
            );
            saved_digests.insert(display, digest);
        }

        let distinct = saved_digests.values().copied().collect::<BTreeSet<_>>();
        assert_eq!(distinct.len(), EXPECTED_PRESET_COUNT);
        eprintln!(
            "Floe saved-state digest: distinct={} / total={}",
            distinct.len(),
            saved_digests.len()
        );

        let a = presets[0].to_string_lossy();
        let b = presets[1].to_string_lossy();
        renderer.set_patch(Some(&a)).unwrap();
        let a_first = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        renderer.set_patch(Some(&b)).unwrap();
        let b_state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        renderer.set_patch(Some(&a)).unwrap();
        let a_again = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        assert_ne!(state_digest(&a_first), state_digest(&b_state));
        assert_eq!(state_digest(&a_first), state_digest(&a_again));

        renderer.plugin_id = "org.example.not-floe".to_string();
        let error = renderer
            .set_patch(Some("file-does-not-need-to-exist.floe-preset"))
            .unwrap_err();
        assert!(error.to_string().contains(FLOE_PLUGIN_ID));
        assert!(!error.to_string().contains("Floe preset を読めない"));
    }

    let startup_preset = &presets[0];
    let cfg = CoreConfig {
        patch_path: Some(startup_preset.to_string_lossy().into_owned()),
        ..test_config_with_plugin_id(FLOE_PLUGIN_ID)
    };
    let mut renderer = RealtimeRenderer::new(&cfg, &entry).unwrap();
    let best_peak = TEST_KEYS
        .into_iter()
        .map(|key| peak(&render_key(&mut renderer, key)))
        .fold(0.0_f32, f32::max);
    assert!(
        best_peak > AUDIBLE_PEAK,
        "起動時 preset が無音: {}",
        startup_preset.display()
    );
}
