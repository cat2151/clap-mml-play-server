//! sforzando の実機 state adapter と出音を確認する統合テスト。

use super::*;
use crate::midi::{MidiEvent, TimedMidiEvent};
use crate::sforzando::SFORZANDO_PLUGIN_ID;

const SFORZANDO_CLAP_ENV: &str = "CMRT_TEST_SFORZANDO_CLAP";
const SFZ_PATCH_A_ENV: &str = "CMRT_TEST_SFORZANDO_PATCH_A";
const SFZ_PATCH_B_ENV: &str = "CMRT_TEST_SFORZANDO_PATCH_B";
const AUDIBLE_PEAK: f32 = 1.0e-6;

fn render_sfz_note(renderer: &mut RealtimeRenderer, key: u8) -> Vec<f32> {
    renderer.reset();
    let mut samples = renderer.render_live_chunk(&[[0x90, key, 110]]).unwrap();
    for _ in 0..63 {
        samples.extend(renderer.render_live_chunk(&[]).unwrap());
    }
    samples.extend(renderer.render_live_chunk(&[[0x80, key, 0]]).unwrap());
    samples
}

#[test]
#[ignore = "実 sforzando CLAP と2つの SFZ 音色が要る"]
fn sforzando_loads_switches_and_renders_sfz_through_state() {
    let clap_path = plugin_path(SFORZANDO_CLAP_ENV);
    let patch_a = plugin_path(SFZ_PATCH_A_ENV);
    let patch_b = plugin_path(SFZ_PATCH_B_ENV);
    let entry = load_entry(&clap_path).unwrap();
    let mut config = test_config_with_plugin_id(SFORZANDO_PLUGIN_ID);
    config.patch_path = Some(patch_a.clone());
    let mut renderer = RealtimeRenderer::new(&config, &entry).unwrap();

    assert_eq!(renderer.plugin_id, SFORZANDO_PLUGIN_ID);
    assert_eq!(renderer.current_patch.as_deref(), Some(patch_a.as_str()));
    let a = render_sfz_note(&mut renderer, 72);
    assert!(peak(&a) > AUDIBLE_PEAK, "最初の SFZ が無音: {patch_a}");

    // 同じ文字列の再選択は current patch を保った no-op。
    renderer.set_patch(Some(&patch_a)).unwrap();
    assert_eq!(renderer.current_patch.as_deref(), Some(patch_a.as_str()));

    renderer.set_patch(Some(&patch_b)).unwrap();
    assert_eq!(renderer.current_patch.as_deref(), Some(patch_b.as_str()));
    let b = render_sfz_note(&mut renderer, 60);
    assert!(peak(&b) > AUDIBLE_PEAK, "2つ目の SFZ が無音: {patch_b}");

    // Known Free Sounds manifests do not register Xylophone.sfz. Mapping must fail before a
    // state call, and the previous program/processor must remain usable.
    let unregistered = std::path::Path::new(&patch_b)
        .parent()
        .map(|parent| parent.join("Xylophone.sfz"));
    if let Some(unregistered) = unregistered.filter(|path| path.is_file()) {
        let unregistered = unregistered.to_string_lossy().into_owned();
        let error = renderer.set_patch(Some(&unregistered)).unwrap_err();
        assert!(format!("{error:#}").contains("program source"), "{error:#}");
        assert_eq!(renderer.current_patch.as_deref(), Some(patch_b.as_str()));
        let after_error = render_sfz_note(&mut renderer, 60);
        assert!(
            peak(&after_error) > AUDIBLE_PEAK,
            "resolution error 後に以前の SFZ が無音"
        );
    }

    renderer.set_patch(Some(&patch_a)).unwrap();
    let a_again = render_sfz_note(&mut renderer, 72);
    assert!(peak(&a_again) > AUDIBLE_PEAK, "最初の SFZ へ戻すと無音");

    let playback = RealtimePlaybackSchedule::new(
        vec![
            TimedMidiEvent {
                sample_pos: 0,
                message: MidiEvent::NoteOn {
                    channel: 0,
                    key: 72,
                    velocity: 110,
                },
            },
            TimedMidiEvent {
                sample_pos: 24_000,
                message: MidiEvent::NoteOff {
                    channel: 0,
                    key: 72,
                    velocity: 0,
                },
            },
        ],
        48_000,
    );
    let offline = render_to_memory(&config, &entry, playback).unwrap();
    assert!(
        peak(&offline) > AUDIBLE_PEAK,
        "offline render の SFZ が無音"
    );

    // loader の照合失敗では、直前に成功した current patch を書き換えない。
    renderer.plugin_id = "org.example.not-sforzando".to_string();
    let error = renderer.set_patch(Some(&patch_b)).unwrap_err();
    assert!(error.to_string().contains(SFORZANDO_PLUGIN_ID));
    assert_eq!(renderer.current_patch.as_deref(), Some(patch_a.as_str()));
}
