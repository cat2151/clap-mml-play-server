//! Surge XT（`org.surge-synth-team.surge-xt`）の実測。
//!
//! 共通のヘルパと環境変数は親モジュールにある。

use clack_extensions::note_ports::NoteDialects;

use super::*;
use crate::pipeline::mml_render_stateless;

/// Surge が CLAP note 経路のままであること。ここが MIDI へ落ちると velocity の量子化が
/// 変わり、既存の出音とディスクキャッシュが一致しなくなる。
///
/// output port が 3 本なのは実測値。host は port 0（main）だけを渡し続ける。
#[test]
#[ignore = "実プラグインが要る"]
fn surge_still_advertises_a_stereo_main_output_and_the_clap_note_dialect() {
    let (descriptor, capabilities) = probe_plugin(SURGE_CLAP_ENV);

    assert_eq!(descriptor.id, "org.surge-synth-team.surge-xt");
    assert_eq!(capabilities.audio_input_ports, 1);
    assert_eq!(capabilities.audio_output_ports, 3);
    assert_eq!(capabilities.main_output_channels, 2);
    assert!(capabilities.main_output_is_main);
    assert_eq!(capabilities.input_note_ports, 1);
    assert!(capabilities
        .input_note_dialects
        .contains(NoteDialects::CLAP));
    assert_eq!(
        resolve_note_dialect(&capabilities, &descriptor).unwrap(),
        NoteEventDialect::Clap
    );
}

/// audio port を capability 駆動へ変えた後も、Surge が input 付きの経路で鳴ること。
///
/// サンプル列そのものの一致は確かめられない。Surge は同一プロセス内で同じ MML を
/// 2 回レンダリングしてもサンプルが一致しない（初期パッチのランダム位相などプラグイン
/// 側の性質で、host の変更とは無関係）。CLAP note 経路のままであることは
/// [`surge_still_advertises_a_stereo_main_output_and_the_clap_note_dialect`] が押さえる。
#[test]
#[ignore = "実プラグインが要る"]
fn surge_still_renders_a_non_silent_offline_mml() {
    let path = plugin_path(SURGE_CLAP_ENV);
    let entry = load_entry(&path).unwrap();

    let samples = mml_render_stateless("t120 o4 l4 c", &test_config(), &entry).unwrap();

    assert!(!samples.is_empty());
    assert!(
        peak(&samples) > 0.0,
        "Surge のオフライン経路が無音になっている"
    );
}

#[test]
#[ignore = "実プラグインが要る"]
fn surge_still_probes_voicing_instead_of_skipping_it() {
    let path = plugin_path(SURGE_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let mut renderer = RealtimeRenderer::new(&test_config(), &entry).unwrap();

    let report = renderer.probe_voicing().unwrap();

    assert!(!report.probe.skipped);
    assert!(report.probe.blocks > 0);
}
