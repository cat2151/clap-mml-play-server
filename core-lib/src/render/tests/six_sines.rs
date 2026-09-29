//! 実 Six Sines CLAP と、GitHub から取得済みの factory 音色を使う統合テスト。
//!
//! plugin の場所は組み込みプロファイル（`[plugins."Six Sines"]` を書かないときの値）、
//! 音色は [`cmrt_server_config::six_sines_factory_dir`] から読む。先に
//! `real_factory_is_downloaded_once` を `--ignored` で走らせて置き場を作っておくこと。

use std::path::PathBuf;

use super::*;
use crate::six_sines::{read_six_sines_voicing, SIX_SINES_PLUGIN_ID};
use crate::PatchVoicing;

const FACTORY_PATCH_COUNT: usize = 216;
const FACTORY_MONO_COUNT: usize = 55;
const AUDIBLE_PEAK: f32 = 1.0e-5;
/// 本番の音色切替（play server の bank worker）と同じ空回しブロック数。
const PATCH_SETTLE_BLOCKS: usize = 4;
/// 音量の包絡を測る窓（interleaved のサンプル数）。
const ENVELOPE_WINDOW: usize = 1024;
/// 同じ音色を別経路で鳴らしたときに許す、包絡の差の RMS / 基準の包絡の RMS。
///
/// 波形そのものは比べない。打鍵ごとに位相や揺らぎが変わる音色（`Pads/Crystal Blue` は
/// 新しい instance どうしでも包絡が 9% ずれる）があり、B を差し替えたときに比較が壊れないよう
/// 音量の時間変化で比べる。B には毎回同じ音になる音色を選んである。
///
/// A→B の B は、plugin が保存する state（全 param）が B 単独と完全に一致しても、
/// 包絡が 2% ずれる（`Bass/Bass 1` → B なら 0%）。param に出ない内部状態が残るためで、
/// その分を許す。
const SAME_PATCH_MAX_ENVELOPE_DIFF: f32 = 0.05;
/// 別の音色だと言えるための、包絡の差の RMS / 基準の包絡の RMS の下限。
const OTHER_PATCH_MIN_ENVELOPE_DIFF: f32 = 0.3;
const PARALLEL_INSTANCES: usize = 8;
const PATCH_A: &str = "Pads/Crystal Blue.sxsnp";
const PATCH_B: &str = "Bass/Bass 2.sxsnp";

fn builtin_profile(name: &str) -> cmrt_server_config::PluginProfile {
    cmrt_server_config::builtin_plugin_profiles()
        .remove(name)
        .unwrap_or_else(|| panic!("組み込みプロファイル '{name}' が無い"))
}

fn six_sines_entry() -> PluginEntry {
    load_entry(&builtin_profile("Six Sines").plugin_path).unwrap()
}

fn six_sines_config() -> CoreConfig {
    test_config_with_plugin_id(
        builtin_profile("Six Sines")
            .plugin_id
            .as_deref()
            .expect("組み込みプロファイルに plugin_id が要る"),
    )
}

fn factory_dir() -> PathBuf {
    cmrt_server_config::six_sines_factory_dir().expect("config の置き場が決まらない")
}

fn factory_patch(display: &str) -> String {
    factory_dir().join(display).to_string_lossy().into_owned()
}

fn factory_patches() -> Vec<PathBuf> {
    let root = factory_dir();
    let patches = crate::collect_patches(&root.to_string_lossy()).unwrap();
    assert_eq!(
        patches.len(),
        FACTORY_PATCH_COUNT,
        "factory 音色の件数が違う: {}",
        root.display()
    );
    patches
}

/// C4 を約 0.5 秒押して離し、約 0.25 秒の余韻まで録る。
fn render_c4(renderer: &mut RealtimeRenderer) -> Vec<f32> {
    let mut samples = renderer.render_live_chunk(&[[0x90, 60, 100]]).unwrap();
    for _ in 0..47 {
        samples.extend(renderer.render_live_chunk(&[]).unwrap());
    }
    samples.extend(renderer.render_live_chunk(&[[0x80, 60, 0]]).unwrap());
    for _ in 0..24 {
        samples.extend(renderer.render_live_chunk(&[]).unwrap());
    }
    samples
}

fn rms(samples: &[f32]) -> f32 {
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len().max(1) as f64).sqrt() as f32
}

fn envelope(samples: &[f32]) -> Vec<f32> {
    samples.chunks(ENVELOPE_WINDOW).map(rms).collect()
}

/// 包絡どうしの `rms(actual - reference) / rms(reference)`。
fn envelope_diff(actual: &[f32], reference: &[f32]) -> f32 {
    assert_eq!(actual.len(), reference.len());
    let (actual, reference) = (envelope(actual), envelope(reference));
    let diff: Vec<f32> = actual.iter().zip(&reference).map(|(a, r)| a - r).collect();
    rms(&diff) / rms(&reference).max(f32::MIN_POSITIVE)
}

/// XML の `<patch … name="…">` の値。
fn patch_name(xml: &str) -> Option<&str> {
    let head = &xml[..xml.find('>')?];
    let at = head.find(" name=\"")? + " name=\"".len();
    let rest = &head[at..];
    Some(&rest[..rest.find('"')?])
}

#[test]
#[ignore = "実 Six Sines CLAP が要る"]
fn the_builtin_profile_picks_the_main_descriptor_out_of_two() {
    let profile = builtin_profile("Six Sines");
    let report =
        probe_plugin_capabilities(&profile.plugin_path, profile.plugin_id.as_deref()).unwrap();

    assert_eq!(report.descriptors.len(), 2);
    assert_eq!(report.selected.id, SIX_SINES_PLUGIN_ID);
    assert_eq!(report.rejected, None);
    assert_eq!(report.main_output_channels, 2);
    assert!(report.extensions.contains(&"clap.state".to_string()));

    let renderer = RealtimeRenderer::new(&six_sines_config(), &six_sines_entry()).unwrap();
    assert_eq!(renderer.plugin_id, SIX_SINES_PLUGIN_ID);
}

/// 216 件すべてを state でロードして C4 を鳴らす。無音の音色は失敗にせず列挙する
/// （MPE 前提や効果音は C4 の 1 打鍵では鳴らないことがある）。
#[test]
#[ignore = "実 Six Sines CLAP と取得済みの factory 音色が要る"]
fn every_factory_patch_loads_as_state() {
    let patches = factory_patches();
    let mut renderer = RealtimeRenderer::new(&six_sines_config(), &six_sines_entry()).unwrap();

    let mut silent = Vec::new();
    let mut name_mismatch = Vec::new();
    for patch in &patches {
        let display = patch.to_string_lossy().into_owned();
        renderer
            .switch_patch(Some(&display), true, PATCH_SETTLE_BLOCKS)
            .unwrap_or_else(|error| panic!("ロードに失敗: {display}: {error:#}"));
        let samples = render_c4(&mut renderer);
        let relative = patch.strip_prefix(factory_dir()).unwrap().display();
        if peak(&samples) <= AUDIBLE_PEAK {
            silent.push(relative.to_string());
        }

        // name 属性の無い音色（`Bells/Detuned Metal`）は、plugin 側で既定名 "Init" になる。
        let file = std::fs::read_to_string(patch).unwrap();
        let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        let state = String::from_utf8_lossy(&state);
        if patch_name(&file).is_some() && patch_name(&file) != patch_name(&state) {
            name_mismatch.push(format!(
                "{relative}: file={:?} state={:?}",
                patch_name(&file),
                patch_name(&state)
            ));
        }
    }

    eprintln!(
        "six-sines loaded={} silent={} {:#?}",
        patches.len(),
        silent.len(),
        silent
    );
    assert!(
        name_mismatch.is_empty(),
        "載った音色の名前がファイルと違う: {name_mismatch:#?}"
    );
}

/// A→B と切り替えた B が、新しい instance で B だけ鳴らした音と同じで、A とは違う。
/// 本番の既定（空回し 4 ブロック）と、空回しなしの経路（scheduled 再生の頭）の両方で確かめる。
/// 空回しなしでも、切り替え直後の 1 音目が鳴ること。
#[test]
#[ignore = "実 Six Sines CLAP と取得済みの factory 音色が要る"]
fn switching_from_a_to_b_sounds_like_b_alone() {
    let entry = six_sines_entry();
    let (a, b) = (factory_patch(PATCH_A), factory_patch(PATCH_B));

    let mut fresh = RealtimeRenderer::new(&six_sines_config(), &entry).unwrap();
    fresh
        .switch_patch(Some(&b), true, PATCH_SETTLE_BLOCKS)
        .unwrap();
    let b_alone = render_c4(&mut fresh);
    let b_alone_state = save_plugin_state(fresh.plugin_instance_mut()).unwrap();
    drop(fresh);

    for settle in [PATCH_SETTLE_BLOCKS, 0] {
        let mut renderer = RealtimeRenderer::new(&six_sines_config(), &entry).unwrap();
        renderer.switch_patch(Some(&a), true, settle).unwrap();
        let a_sound = render_c4(&mut renderer);
        renderer.switch_patch(Some(&b), true, settle).unwrap();
        let b_after_a = render_c4(&mut renderer);
        let b_after_a_state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        assert!(
            b_after_a_state == b_alone_state,
            "settle={settle}: A→B の state が B 単独の state と違う（A の値が残っている）"
        );

        let same = envelope_diff(&b_after_a, &b_alone);
        let other = envelope_diff(&a_sound, &b_alone);
        eprintln!(
            "six-sines settle={settle} rms(B alone)={:.5} rms(B after A)={:.5} rms(A)={:.5} diff(B after A, B)={same:.5} diff(A, B)={other:.5}",
            rms(&b_alone),
            rms(&b_after_a),
            rms(&a_sound),
        );
        assert!(peak(&b_alone) > AUDIBLE_PEAK, "B が無音");
        assert!(peak(&a_sound) > AUDIBLE_PEAK, "settle={settle}: A が無音");
        assert!(
            peak(&b_after_a) > AUDIBLE_PEAK,
            "settle={settle}: A→B の B が無音"
        );
        assert!(
            same <= SAME_PATCH_MAX_ENVELOPE_DIFF,
            "settle={settle}: A→B の B が B 単独と違う（{same}）"
        );
        assert!(
            other >= OTHER_PATCH_MIN_ENVELOPE_DIFF,
            "settle={settle}: A と B の出音が近すぎる（{other}）"
        );
    }
}

/// config の `patch_path` に `.sxsnp` を書いた起動（`activate()` 前のロード）も同じ経路を通る。
#[test]
#[ignore = "実 Six Sines CLAP と取得済みの factory 音色が要る"]
fn a_six_sines_patch_in_the_config_is_loaded_before_activate() {
    let patch = factory_patch(PATCH_A);
    let cfg = CoreConfig {
        patch_path: Some(patch.clone()),
        ..six_sines_config()
    };
    let mut renderer = RealtimeRenderer::new(&cfg, &six_sines_entry()).unwrap();

    assert!(
        peak(&render_c4(&mut renderer)) > AUDIBLE_PEAK,
        "無音: {patch}"
    );
    let file = std::fs::read_to_string(&patch).unwrap();
    let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
    assert_eq!(
        patch_name(&String::from_utf8_lossy(&state)),
        patch_name(&file)
    );
}

/// `.sxsnp` は Surge XT へ、`.fxp` は Six Sines へ、送る前に拒む（set_patch と起動時の両方）。
#[test]
#[ignore = "実 Six Sines CLAP と実 Surge XT CLAP が要る"]
fn patches_of_the_other_plugin_are_refused_before_they_are_sent() {
    let surge_entry = load_entry(&builtin_profile("Surge XT").plugin_path).unwrap();
    let mut surge = RealtimeRenderer::new(&test_config(), &surge_entry).unwrap();
    let error = surge.set_patch(Some("does-not-matter.sxsnp")).unwrap_err();
    assert!(error.to_string().contains(SIX_SINES_PLUGIN_ID), "{error:#}");
    let cfg = CoreConfig {
        patch_path: Some("does-not-matter.sxsnp".to_string()),
        ..test_config()
    };
    let error = RealtimeRenderer::new(&cfg, &surge_entry)
        .err()
        .expect("Surge が '.sxsnp' で立ち上がってしまった");
    assert!(error.to_string().contains(SIX_SINES_PLUGIN_ID), "{error:#}");

    let entry = six_sines_entry();
    let mut six_sines = RealtimeRenderer::new(&six_sines_config(), &entry).unwrap();
    let error = six_sines
        .set_patch(Some("does-not-matter.fxp"))
        .unwrap_err();
    assert!(error.to_string().contains(SIX_SINES_PLUGIN_ID), "{error:#}");
    let cfg = CoreConfig {
        patch_path: Some("does-not-matter.fxp".to_string()),
        ..six_sines_config()
    };
    let error = RealtimeRenderer::new(&cfg, &entry)
        .err()
        .expect("Six Sines が '.fxp' で立ち上がってしまった");
    assert!(error.to_string().contains(SIX_SINES_PLUGIN_ID), "{error:#}");
}

/// 取得済み 216 件の mono/poly を param 523 から読む。読めない音色は 1 件も無い。
#[test]
#[ignore = "取得済みの factory 音色が要る"]
fn factory_voicing_is_read_from_the_play_mode_param() {
    let patches = factory_patches();
    let voicings: Vec<PatchVoicing> = patches
        .iter()
        .map(|path| read_six_sines_voicing(path))
        .collect();
    let count = |wanted| voicings.iter().filter(|v| **v == wanted).count();

    assert_eq!(count(PatchVoicing::Unknown), 0);
    assert_eq!(count(PatchVoicing::Mono), FACTORY_MONO_COUNT);
    assert_eq!(
        count(PatchVoicing::Poly),
        FACTORY_PATCH_COUNT - FACTORY_MONO_COUNT
    );
}

/// instance を 8 本同時に作っても落ちない（落ちるなら直列化の一覧に足す）。
#[test]
#[ignore = "実 Six Sines CLAP が要る"]
fn instances_can_be_created_in_parallel() {
    let entry = six_sines_entry();
    let cfg = six_sines_config();
    let specs = vec![
        RendererSpec {
            cfg: &cfg,
            entry: &entry,
        };
        PARALLEL_INSTANCES
    ];

    let mut renderers = create_renderers_parallel(&specs, PARALLEL_INSTANCES, &|_| {}).unwrap();

    assert_eq!(renderers.len(), PARALLEL_INSTANCES);
    assert!(!plugin_requires_serial_instantiation(SIX_SINES_PLUGIN_ID));
    for renderer in &mut renderers {
        assert!(peak(&render_c4(renderer)) > AUDIBLE_PEAK);
    }
}
