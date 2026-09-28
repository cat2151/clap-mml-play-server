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

const ARIAX_ENV: &str = "CMRT_TEST_SFORZANDO_ARIAX";
const TABLEWARP2_SFZ_ENV: &str = "CMRT_TEST_SFORZANDO_TABLEWARP2";
const ARIAX_KEY: u8 = 72;
/// 同じ patch の 2 回の render の差（対照）に対し、`.ariax` の音との差が何倍あれば
/// 「Param が音に効いた」とみなすか。
const ARIAX_DIFF_OVER_CONTROL: f32 = 10.0;
/// 対照が 0 に近いときでも、丸め誤差程度の差を「効いた」とみなさないための下限。
const ARIAX_MIN_DIFF_RMS: f32 = 1.0e-3;

fn rms_of_difference(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().max(b.len());
    let sum: f64 = (0..len)
        .map(|index| {
            let diff = a.get(index).copied().unwrap_or(0.0) - b.get(index).copied().unwrap_or(0.0);
            f64::from(diff) * f64::from(diff)
        })
        .sum();
    (sum / len.max(1) as f64).sqrt() as f32
}

fn rms(samples: &[f32]) -> f32 {
    rms_of_difference(samples, &[])
}

fn load_state_between_blocks(renderer: &mut RealtimeRenderer, state: &[u8]) {
    let stopped = renderer.processor.take().unwrap().stop_processing();
    let loaded = load_plugin_state(renderer.plugin_instance_mut(), state);
    renderer.processor = Some(stopped.start_processing().unwrap());
    loaded.unwrap();
}

fn slot_params(xml: &str) -> Vec<(String, f64)> {
    let root = xmltree::Element::parse(std::io::Cursor::new(xml.as_bytes())).unwrap();
    let slot = root.get_child("Slot").expect("AriaSave に Slot が無い");
    slot.children
        .iter()
        .filter_map(|node| match node {
            xmltree::XMLNode::Element(param) if param.name == "Param" => Some((
                param.attributes["id"].clone(),
                param.attributes["value"].parse().unwrap(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
#[ignore = "実 sforzando CLAP と TableWarp2 の .sfz / .ariax が要る"]
fn ariax_params_change_the_sound_and_survive_a_state_save() {
    let clap_path = plugin_path(SFORZANDO_CLAP_ENV);
    let ariax_path = plugin_path(ARIAX_ENV);
    let tablewarp2 = plugin_path(TABLEWARP2_SFZ_ENV);
    let ariax_xml = std::fs::read_to_string(&ariax_path).unwrap();
    let entry = load_entry(&clap_path).unwrap();
    let mut config = test_config_with_plugin_id(SFORZANDO_PLUGIN_ID);
    config.patch_path = Some(tablewarp2.clone());
    let mut renderer = RealtimeRenderer::new(&config, &entry).unwrap();

    // 1. 対照: 同じ `.sfz` で同じ note を 2 回。
    let sfz_first = render_sfz_note(&mut renderer, ARIAX_KEY);
    let sfz_second = render_sfz_note(&mut renderer, ARIAX_KEY);
    // state の load をやり直すだけで音が変わらないことも対照に含める。
    let program =
        crate::sforzando::resolve_sforzando_program(std::path::Path::new(&tablewarp2)).unwrap();
    let sfz_state = crate::sforzando::sforzando_state_blob(
        renderer.init_state.as_deref().unwrap(),
        &program,
        crate::sforzando::SfzStreaming::PluginDefault,
    )
    .unwrap();
    load_state_between_blocks(&mut renderer, &sfz_state);
    let sfz_reloaded = render_sfz_note(&mut renderer, ARIAX_KEY);
    let control = rms_of_difference(&sfz_first, &sfz_second)
        .max(rms_of_difference(&sfz_first, &sfz_reloaded));

    // 2. `.ariax` の state を load して同じ note。
    let state = crate::sforzando::sforzando_ariax_state_blob(
        renderer.init_state.as_deref().unwrap(),
        &ariax_xml,
        crate::sforzando::SfzStreaming::PluginDefault,
    )
    .unwrap();
    load_state_between_blocks(&mut renderer, &state);
    let ariax = render_sfz_note(&mut renderer, ARIAX_KEY);
    let ariax_diff = rms_of_difference(&sfz_first, &ariax);
    eprintln!(
        "ariax: sfz_rms={} ariax_rms={} ariax_peak={} repeat_diff_rms={} reload_diff_rms={} ariax_diff_rms={ariax_diff}",
        rms(&sfz_first),
        rms(&ariax),
        peak(&ariax),
        rms_of_difference(&sfz_first, &sfz_second),
        rms_of_difference(&sfz_first, &sfz_reloaded),
    );
    assert!(
        peak(&ariax) > AUDIBLE_PEAK,
        ".ariax の音が無音: {ariax_path}"
    );
    assert!(
        ariax_diff > ARIAX_MIN_DIFF_RMS && ariax_diff > control * ARIAX_DIFF_OVER_CONTROL,
        ".ariax の Param が音に効いていない: control={control} ariax_diff={ariax_diff}"
    );

    // 3. load 後に plugin から保存した state に `.ariax` の Param が残る。
    let saved = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
    let saved_params = slot_params(&crate::sforzando::codec::decode(&saved).unwrap());
    let preset_params = slot_params(&ariax_xml);
    let missing: Vec<_> = preset_params
        .iter()
        .filter(|(id, value)| {
            !saved_params
                .iter()
                .any(|(saved_id, saved)| saved_id == id && (saved - value).abs() < 1.0e-6)
        })
        .collect();
    eprintln!(
        "ariax: preset_params={} saved_params={} missing={missing:?}",
        preset_params.len(),
        saved_params.len()
    );
    let (_, param_73) = preset_params
        .iter()
        .find(|(id, _)| id == "73")
        .expect(".ariax に Param id=73 が無い");
    assert!(
        saved_params
            .iter()
            .any(|(id, value)| id == "73" && (value - param_73).abs() < 1.0e-6),
        "保存 state に Param 73={param_73} が無い: {saved_params:?}"
    );
    assert!(
        missing.is_empty(),
        "保存 state から消えた Param: {missing:?}"
    );
}

/// `.ariax` の Slot の bankId だけを ARIA に無い値へ変えた preset を、本物の bank manifest と
/// `.sfz` の複製の隣に置く。座標の照合だけで弾かれることを確かめるための fixture。
struct MismatchedAriaxBank(std::path::PathBuf);

impl MismatchedAriaxBank {
    fn new(tablewarp2_sfz: &str, ariax_xml: &str) -> Self {
        let sfz = std::path::Path::new(tablewarp2_sfz);
        let programs = sfz.parent().unwrap();
        let bank_dir = programs.parent().unwrap();
        let root = std::env::temp_dir().join(format!(
            "cmrt_sforzando_mismatched_ariax_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let copied_programs = root.join(programs.file_name().unwrap());
        std::fs::create_dir_all(&copied_programs).unwrap();
        std::fs::copy(sfz, copied_programs.join(sfz.file_name().unwrap())).unwrap();
        for entry in std::fs::read_dir(bank_dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".bank.xml") {
                std::fs::copy(entry.path(), root.join(entry.file_name())).unwrap();
            }
        }
        let root_element =
            xmltree::Element::parse(std::io::Cursor::new(ariax_xml.as_bytes())).unwrap();
        let bank_id = &root_element.get_child("Slot").unwrap().attributes["bankId"];
        let mismatched = ariax_xml.replacen(&format!(r#"bankId="{bank_id}""#), r#"bankId="-1""#, 1);
        assert_ne!(mismatched, ariax_xml, ".ariax の bankId を書き換えられない");
        std::fs::write(root.join("Mismatched.ariax"), mismatched).unwrap();
        Self(root)
    }

    fn ariax(&self) -> String {
        self.0
            .join("Mismatched.ariax")
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for MismatchedAriaxBank {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "実 sforzando CLAP と TableWarp2 の .sfz / .ariax が要る"]
fn ariax_switches_with_sfz_and_a_mismatched_ariax_keeps_the_previous_patch() {
    let clap_path = plugin_path(SFORZANDO_CLAP_ENV);
    let ariax_path = plugin_path(ARIAX_ENV);
    let tablewarp2 = plugin_path(TABLEWARP2_SFZ_ENV);
    let entry = load_entry(&clap_path).unwrap();
    let mut config = test_config_with_plugin_id(SFORZANDO_PLUGIN_ID);
    config.patch_path = Some(tablewarp2.clone());
    let mut renderer = RealtimeRenderer::new(&config, &entry).unwrap();

    // .sfz → .ariax → .sfz
    let sfz = render_sfz_note(&mut renderer, ARIAX_KEY);
    assert!(
        peak(&sfz) > AUDIBLE_PEAK,
        "最初の .sfz が無音: {tablewarp2}"
    );
    renderer.set_patch(Some(&ariax_path)).unwrap();
    assert_eq!(renderer.current_patch.as_deref(), Some(ariax_path.as_str()));
    let ariax = render_sfz_note(&mut renderer, ARIAX_KEY);
    assert!(peak(&ariax) > AUDIBLE_PEAK, ".ariax が無音: {ariax_path}");
    let sfz_vs_ariax = rms_of_difference(&sfz, &ariax);
    assert!(
        sfz_vs_ariax > ARIAX_MIN_DIFF_RMS,
        "set_patch の .ariax が .sfz と同じ音: diff={sfz_vs_ariax}"
    );

    // 座標が manifest に無い .ariax は load 前に弾かれ、直前の .ariax が鳴り続ける。
    let ariax_xml = std::fs::read_to_string(&ariax_path).unwrap();
    let mismatched = MismatchedAriaxBank::new(&tablewarp2, &ariax_xml);
    let error = renderer.set_patch(Some(&mismatched.ariax())).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("Slot 座標"), "{message}");
    assert!(message.contains("bankId='-1'"), "{message}");
    assert_eq!(renderer.current_patch.as_deref(), Some(ariax_path.as_str()));
    let after_error = render_sfz_note(&mut renderer, ARIAX_KEY);
    let after_vs_ariax = rms_of_difference(&ariax, &after_error);
    let after_vs_sfz = rms_of_difference(&sfz, &after_error);
    eprintln!(
        "ariax switch: sfz_vs_ariax={sfz_vs_ariax} after_error_vs_ariax={after_vs_ariax} after_error_vs_sfz={after_vs_sfz}"
    );
    assert!(
        peak(&after_error) > AUDIBLE_PEAK,
        "拒否の後に直前の .ariax が無音"
    );
    assert!(
        after_vs_ariax * ARIAX_DIFF_OVER_CONTROL < after_vs_sfz,
        "拒否の後に直前の .ariax の音ではない: vs_ariax={after_vs_ariax} vs_sfz={after_vs_sfz}"
    );

    renderer.set_patch(Some(&tablewarp2)).unwrap();
    let sfz_again = render_sfz_note(&mut renderer, ARIAX_KEY);
    assert!(peak(&sfz_again) > AUDIBLE_PEAK, ".sfz へ戻すと無音");
    let back_vs_sfz = rms_of_difference(&sfz, &sfz_again);
    eprintln!("ariax switch: back_to_sfz_vs_sfz={back_vs_sfz}");
    assert!(
        back_vs_sfz * ARIAX_DIFF_OVER_CONTROL < sfz_vs_ariax,
        ".sfz へ戻した音が最初の .sfz と違う: back_vs_sfz={back_vs_sfz} sfz_vs_ariax={sfz_vs_ariax}"
    );

    // 起動時の patch が .ariax でも、realtime と offline の両方で鳴る。
    let mut config = test_config_with_plugin_id(SFORZANDO_PLUGIN_ID);
    config.patch_path = Some(ariax_path.clone());
    let mut initial = RealtimeRenderer::new(&config, &entry).unwrap();
    let initial_ariax = render_sfz_note(&mut initial, ARIAX_KEY);
    assert!(
        peak(&initial_ariax) > AUDIBLE_PEAK,
        "起動時の .ariax が無音"
    );
    let playback = RealtimePlaybackSchedule::new(
        vec![
            TimedMidiEvent {
                sample_pos: 0,
                message: MidiEvent::NoteOn {
                    channel: 0,
                    key: ARIAX_KEY,
                    velocity: 110,
                },
            },
            TimedMidiEvent {
                sample_pos: 24_000,
                message: MidiEvent::NoteOff {
                    channel: 0,
                    key: ARIAX_KEY,
                    velocity: 0,
                },
            },
        ],
        48_000,
    );
    let offline = render_to_memory(&config, &entry, playback).unwrap();
    assert!(
        peak(&offline) > AUDIBLE_PEAK,
        "offline render の .ariax が無音"
    );
}
