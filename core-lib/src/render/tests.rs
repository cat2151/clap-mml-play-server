//! 実プラグインを読み込む統合テスト。
//!
//! CLAP 本体が要るので通常の `cargo test` では走らない（すべて `#[ignore]`）。
//! 走らせるにはプラグインのパスを環境変数で渡す:
//!
//! ```text
//! CMRT_TEST_DEXED_CLAP=C:\Program Files\Common Files\CLAP\Dexed.clap
//! CMRT_TEST_SURGE_CLAP=C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap
//! cargo test -p cmrt-core -- --ignored
//! ```
//!
//! 環境変数が無いテストは、黙って通さず panic させる（未検証を成功と誤認しないため）。

use clack_extensions::note_ports::NoteDialects;

use super::*;
use crate::host::load_entry;
use crate::pipeline::mml_render_stateless;

const DEXED_CLAP_ENV: &str = "CMRT_TEST_DEXED_CLAP";
const SURGE_CLAP_ENV: &str = "CMRT_TEST_SURGE_CLAP";
const SAMPLE_RATE: f64 = 48_000.0;
const BUFFER_SIZE: usize = 512;
/// 報告書の probe と同じ条件（8 worker で 16 instance）。
const LIVE_INSTANCE_COUNT: usize = 16;
const BUILD_THREADS: usize = 8;

fn plugin_path(env: &str) -> String {
    std::env::var(env)
        .unwrap_or_else(|_| panic!("{env} に CLAP のパスを設定してからこのテストを実行すること"))
}

fn test_config() -> CoreConfig {
    CoreConfig {
        output_midi: String::new(),
        output_wav: String::new(),
        sample_rate: SAMPLE_RATE,
        buffer_size: BUFFER_SIZE,
        patch_path: None,
        patches_dir: None,
        random_patch: false,
        ..Default::default()
    }
}

fn test_config_with_plugin_id(plugin_id: &str) -> CoreConfig {
    CoreConfig {
        plugin_id: Some(plugin_id.to_string()),
        ..test_config()
    }
}

const DEXED_PLUGIN_ID: &str = "com.digital-suburban.dexed";

/// descriptor 選択から capability probe までを、instance を 1 つ作って通す。
fn probe_plugin(env: &str) -> (SelectedDescriptor, PluginCapabilities) {
    let path = plugin_path(env);
    let entry = load_entry(&path).unwrap();
    let descriptor = select_descriptor(&entry, None).unwrap();
    let mut plugin_instance = create_plugin_instance_without_patch(&entry, &descriptor).unwrap();
    let capabilities = probe_capabilities(&mut plugin_instance, &descriptor).unwrap();
    (descriptor, capabilities)
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

/// Note On → 9 block → Note Off。報告書の live probe と同じ手順。
fn render_live_note(renderer: &mut RealtimeRenderer) -> Vec<f32> {
    let mut samples = renderer.render_live_chunk(&[[0x90, 60, 100]]).unwrap();
    for _ in 0..9 {
        samples.extend(renderer.render_live_chunk(&[]).unwrap());
    }
    samples.extend(renderer.render_live_chunk(&[[0x80, 60, 0]]).unwrap());
    samples
}

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

/// `CoreConfig.plugin_id` が instance 生成側の descriptor 選択まで届くこと。
///
/// ここが届かないと、descriptor を複数持つ CLAP で「2 件あり決められない」と落ちる
/// （起動ログだけは `plugin_id` で 1 件に決まるので、食い違いが見えにくい）。
#[test]
#[ignore = "実プラグインが要る"]
fn live_instance_creation_honors_the_configured_plugin_id() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = test_config_with_plugin_id(DEXED_PLUGIN_ID);

    let mut renderer = RealtimeRenderer::new(&cfg, &entry).unwrap();
    let samples = render_live_note(&mut renderer);

    assert!(peak(&samples) > 0.0, "ライブ経路が無音になっている");
}

#[test]
#[ignore = "実プラグインが要る"]
fn offline_instance_creation_honors_the_configured_plugin_id() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = test_config_with_plugin_id(DEXED_PLUGIN_ID);

    let samples = mml_render_stateless("t120 o4 l4 c", &cfg, &entry).unwrap();

    assert!(peak(&samples) > 0.0, "オフライン経路が無音になっている");
}

/// config の `plugin_id` が CLAP の中身と食い違うときは、黙って別の音色で鳴らさず落とす。
/// エラーには実際にあった descriptor ID を出す（config の書き間違いと CLAP の
/// 入れ替わりを区別できるようにするため）。
#[test]
#[ignore = "実プラグインが要る"]
fn live_instance_creation_rejects_a_plugin_id_the_clap_does_not_have() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = test_config_with_plugin_id("com.example.not-installed");

    let Err(error) = RealtimeRenderer::new(&cfg, &entry) else {
        panic!("plugin_id が食い違うのに instance が作れてしまった");
    };
    let error = error.to_string();

    assert!(error.contains("com.example.not-installed"), "{error}");
    assert!(error.contains(DEXED_PLUGIN_ID), "{error}");
}

#[test]
#[ignore = "実プラグインが要る"]
fn offline_instance_creation_rejects_a_plugin_id_the_clap_does_not_have() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = test_config_with_plugin_id("com.example.not-installed");

    let error = mml_render_stateless("t120 o4 l4 c", &cfg, &entry)
        .unwrap_err()
        .to_string();

    assert!(error.contains("com.example.not-installed"), "{error}");
    assert!(error.contains(DEXED_PLUGIN_ID), "{error}");
}

mod cartridge;
