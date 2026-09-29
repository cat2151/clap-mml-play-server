//! 実 TyrellN6 CLAP へ、本番と同じ経路（`switch_patch` と `activate()` 前のロード）で `.h2p` を流す。
//!
//! TyrellN6 は同じ音色でも打鍵の履歴で包絡がずれる（[`super::tyrelln6`] の
//! `the_same_patch_does_not_repeat_after_played_notes`）。だから A→B の B は「新しい instance の
//! B 単独と一致」ではなく、state が B→B と一致すること・A より B 単独に近いことで確かめる。

use super::tyrelln6::{
    envelope_diff, preset, presets_dir, render_c4, rms, tyrelln6_entry, tyrelln6_renderer, LoadRun,
    AUDIBLE_PEAK, IDLE_BLOCKS_BEFORE_LOAD, OTHER_PATCH_MIN_ENVELOPE_DIFF, PATCH_A, PATCH_B,
    SETTLED_MAX_ENVELOPE_DIFF, SETTLE_PROBE_PATCHES,
};
use super::*;
use cmrt_server_config::TYRELLN6_PLUGIN_ID;

const FACTORY_PATCH_COUNT: usize = 669;
/// 本番の音色切替（play server の bank worker）と同じ空回しブロック数。
const PATCH_SETTLE_BLOCKS: usize = 4;

fn preset_path(display: &str) -> String {
    presets_dir().join(display).to_string_lossy().into_owned()
}

/// 669 件すべてを `switch_patch` でロードして C4 を鳴らす。無音の音色は失敗にせず列挙する
/// （`Hits` `Loops` `FX` は C4 の 1 打鍵では鳴らないことがある）。
#[test]
#[ignore = "実 TyrellN6 CLAP と音色置き場が要る"]
fn every_factory_patch_loads_through_switch_patch() {
    let root = presets_dir();
    let patches = crate::collect_patches(&root.to_string_lossy()).unwrap();
    assert_eq!(
        patches.len(),
        FACTORY_PATCH_COUNT,
        "音色の件数が違う: {}",
        root.display()
    );
    let mut renderer = tyrelln6_renderer(&tyrelln6_entry());

    let mut failed = Vec::new();
    let mut silent = Vec::new();
    for patch in &patches {
        let display = crate::to_relative(&root.to_string_lossy(), patch);
        if let Err(error) =
            renderer.switch_patch(Some(&patch.to_string_lossy()), true, PATCH_SETTLE_BLOCKS)
        {
            failed.push(format!("{display}: {error:#}"));
            continue;
        }
        if peak(&render_c4(&mut renderer)) <= AUDIBLE_PEAK {
            silent.push(display);
        }
    }

    eprintln!(
        "tyrelln6 loaded={} failed={} silent={} {silent:#?}",
        patches.len() - failed.len(),
        failed.len(),
        silent.len(),
    );
    assert!(failed.is_empty(), "ロードに失敗: {failed:#?}");
}

/// A→B（`switch_patch` 経由）の B は、B→B（同じ打鍵回数のあと B を流し直す）と state がバイト一致し、
/// 無音でなく、A より B 単独に近い。本番の既定（空回し 4）と空回しなし（scheduled 再生の頭）の両方。
#[test]
#[ignore = "実 TyrellN6 CLAP と音色置き場が要る"]
fn switching_from_a_to_b_leaves_no_state_of_a() {
    let entry = tyrelln6_entry();
    let (a, b) = (preset_path(PATCH_A), preset_path(PATCH_B));
    // 同じ綴りは `set_patch` が省くので、B→B は区切り文字だけ変えた綴りで流し直す。
    let b_respelled = b.replace('/', "\\");
    assert_ne!(b, b_respelled);

    for settle in [PATCH_SETTLE_BLOCKS, 0] {
        let mut alone = tyrelln6_renderer(&entry);
        alone.switch_patch(Some(&b), true, settle).unwrap();
        let b_alone = render_c4(&mut alone);
        drop(alone);

        let mut renderer = tyrelln6_renderer(&entry);
        renderer.switch_patch(Some(&a), true, settle).unwrap();
        let a_sound = render_c4(&mut renderer);
        renderer.switch_patch(Some(&b), true, settle).unwrap();
        let b_after_a = render_c4(&mut renderer);
        let b_after_a_state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        drop(renderer);

        let mut renderer = tyrelln6_renderer(&entry);
        renderer.switch_patch(Some(&b), true, settle).unwrap();
        render_c4(&mut renderer);
        renderer
            .switch_patch(Some(&b_respelled), true, settle)
            .unwrap();
        let b_after_b = render_c4(&mut renderer);
        let b_after_b_state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        drop(renderer);

        let same = envelope_diff(&b_after_a, &b_alone);
        let history = envelope_diff(&b_after_b, &b_alone);
        let other = envelope_diff(&a_sound, &b_alone);
        eprintln!(
            "tyrelln6 switch settle={settle}: state(A→B)==state(B→B)={} rms(B after A)={:.5} diff(A→B, B alone)={same:.5} diff(B→B, B alone)={history:.5} diff(A, B alone)={other:.5}",
            b_after_a_state == b_after_b_state,
            rms(&b_after_a),
        );

        assert!(
            b_after_a_state == b_after_b_state,
            "settle={settle}: A→B の state が B→B と違う（A の値が残っている）"
        );
        assert!(
            peak(&b_after_a) > AUDIBLE_PEAK,
            "settle={settle}: A→B の B が無音"
        );
        assert!(
            other >= OTHER_PATCH_MIN_ENVELOPE_DIFF,
            "settle={settle}: A と B の出音が近すぎる（{other}）"
        );
        assert!(
            same < other,
            "settle={settle}: A→B の B が A に近い（{same} >= {other}）"
        );
    }
}

/// `process()` を回したあとの instance へ空回しなしで流しても、1 音目は生成直後に流した
/// 1 音目と同じに鳴る（ロード直後の空回しで反映を寄せきっている）。`reset_before` の両方で。
#[test]
#[ignore = "実 TyrellN6 CLAP と音色置き場が要る"]
fn the_first_note_after_switching_without_settle_is_already_the_new_patch() {
    let entry = tyrelln6_entry();
    let mut worst = 0.0_f32;
    for patch in SETTLE_PROBE_PATCHES {
        let (fresh, _) = LoadRun::FRESH.render(&entry, &preset(patch));
        for reset_before in [true, false] {
            let mut renderer = tyrelln6_renderer(&entry);
            for _ in 0..IDLE_BLOCKS_BEFORE_LOAD {
                renderer.render_live_chunk(&[]).unwrap();
            }
            renderer
                .switch_patch(Some(&preset_path(patch)), reset_before, 0)
                .unwrap();
            let diff = envelope_diff(&render_c4(&mut renderer), &fresh);
            eprintln!("tyrelln6 settle=0 reset_before={reset_before} {patch}: diff(first note, fresh load)={diff:.5}");
            worst = worst.max(diff);
        }
    }

    assert!(
        worst <= SETTLED_MAX_ENVELOPE_DIFF,
        "空回しなしの 1 音目が反映しきっていない（{worst}）"
    );
}

/// config の `patch_path` に `.h2p` を書いた起動（`activate()` 前のロード）でも 1 音目から
/// その音色で鳴る。
#[test]
#[ignore = "実 TyrellN6 CLAP と音色置き場が要る"]
fn a_tyrelln6_patch_in_the_config_is_loaded_before_activate() {
    let entry = tyrelln6_entry();
    let cfg = CoreConfig {
        patch_path: Some(preset_path(PATCH_A)),
        ..test_config_with_plugin_id(TYRELLN6_PLUGIN_ID)
    };
    let mut renderer = RealtimeRenderer::new(&cfg, &entry).unwrap();
    let first_note = render_c4(&mut renderer);
    drop(renderer);
    let (fresh, _) = LoadRun::FRESH.render(&entry, &preset(PATCH_A));

    let diff = envelope_diff(&first_note, &fresh);
    eprintln!(
        "tyrelln6 before activate {PATCH_A}: rms={:.5} diff(first note, fresh load)={diff:.5}",
        rms(&first_note)
    );
    assert!(peak(&first_note) > AUDIBLE_PEAK, "無音: {PATCH_A}");
    assert!(
        diff <= SETTLED_MAX_ENVELOPE_DIFF,
        "音色が載っていない（{diff}）"
    );
}
