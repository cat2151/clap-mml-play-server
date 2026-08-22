//! Dexed（`com.digital-suburban.dexed`）の実測。
//!
//! 共通のヘルパと環境変数は親モジュールにある。

use clack_extensions::note_ports::NoteDialects;

use super::*;
use crate::pipeline::mml_render_stateless;

/// 報告書の probe と同じ条件（8 worker で 16 instance）。
const LIVE_INSTANCE_COUNT: usize = 16;
const BUILD_THREADS: usize = 8;

#[test]
#[ignore = "実プラグインが要る"]
fn dexed_exposes_a_single_known_descriptor() {
    let (descriptor, _) = probe_plugin(DEXED_CLAP_ENV);

    assert_eq!(descriptor.id, "com.digital-suburban.dexed");
    assert_eq!(descriptor.name, "Dexed");
    assert_eq!(descriptor.vendor, "Digital Suburban");
    assert_eq!(descriptor.version, "1.0.1");
}

#[test]
#[ignore = "実プラグインが要る"]
fn dexed_has_no_audio_input_and_only_the_midi_note_dialect() {
    let (descriptor, capabilities) = probe_plugin(DEXED_CLAP_ENV);

    assert_eq!(capabilities.audio_input_ports, 0);
    assert_eq!(capabilities.audio_output_ports, 1);
    assert_eq!(capabilities.main_output_channels, 2);
    assert!(capabilities.main_output_is_main);
    assert_eq!(capabilities.input_note_ports, 1);
    assert_eq!(capabilities.input_note_dialects, NoteDialects::MIDI);
    assert_eq!(
        resolve_note_dialect(&capabilities, &descriptor).unwrap(),
        NoteEventDialect::Midi
    );
}

#[test]
#[ignore = "実プラグインが要る"]
fn dexed_renders_a_non_silent_offline_mml() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();

    let samples = mml_render_stateless("t120 o4 l4 c", &test_config(), &entry).unwrap();

    assert!(!samples.is_empty());
    assert!(peak(&samples) > 0.0, "オフライン経路が無音になっている");
}

#[test]
#[ignore = "実プラグインが要る"]
fn dexed_renders_non_silent_live_midi() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let mut renderer = RealtimeRenderer::new(&test_config(), &entry).unwrap();

    let samples = render_live_note(&mut renderer);

    assert_eq!(samples.len(), BUFFER_SIZE * 2 * 11);
    assert!(peak(&samples) > 0.0, "live 経路が無音になっている");
}

/// `parallel.rs` の unsafe な thread handoff は Dexed について保証が無く、実測でしか
/// 確かめられない。plugin / version 別の回帰対象としてここに固定する。
#[test]
#[ignore = "実プラグインが要る"]
fn dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();

    let cfg = test_config();
    let specs = vec![
        RendererSpec {
            cfg: &cfg,
            entry: &entry
        };
        LIVE_INSTANCE_COUNT
    ];
    let mut renderers = create_renderers_parallel(&specs, BUILD_THREADS, &|_| {}).unwrap();

    assert_eq!(renderers.len(), LIVE_INSTANCE_COUNT);
    for renderer in &mut renderers {
        let samples = render_live_note(renderer);
        assert!(peak(&samples) > 0.0, "移送後の instance が無音になっている");
    }
    // 破棄（deactivate）まで通ることが確認したい本体なので、明示的に落とす。
    drop(renderers);
}

/// MIDI dialect には note_id が無く NOTE_END が返らないので、probe を走らせると
/// 「判定できなかった」が「Poly と判定した」と区別できなくなる。走らせないことを固定する。
#[test]
#[ignore = "実プラグインが要る"]
fn dexed_skips_the_voicing_probe_instead_of_reporting_an_unmeasured_poly() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let mut renderer = RealtimeRenderer::new(&test_config(), &entry).unwrap();

    let report = renderer.probe_voicing().unwrap();

    assert!(report.probe.skipped);
    assert_eq!(report.probe.blocks, 0);
    assert!(report.probe.ended_note_ids.is_empty());
    assert_eq!(report.decision, crate::voicing::PatchVoicing::Poly);
}
