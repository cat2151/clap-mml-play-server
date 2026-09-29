//! 実 TyrellN6 CLAP の descriptor と state の形式を測り、`.h2p` を CLAP state として
//! 流し込めるかを確かめる。
//!
//! plugin は組み込みプロファイル `TyrellN6` から、音色置き場（`<DataPath>\Presets\TyrellN6`）は
//! 本番と同じ経路で registry から取る（[`presets_dir`]）。
//!
//! 本番の音色切替（`switch_patch`）を通すテストは [`super::tyrelln6_patch_switch`] にある。
//! ここは state を直に流して plugin の性質を測るものと、両方が使うヘルパを置く。

use std::collections::BTreeSet;
use std::path::PathBuf;

use cmrt_server_config::TYRELLN6_PLUGIN_ID;

use super::*;

pub(super) const PATCH_A: &str = "01 Basses/Abgrund.h2p";
pub(super) const PATCH_B: &str = "06 Pads/Airy Pad.h2p";
/// 同じ音色でも打鍵の履歴で音が変わることを見る音色。新しい instance どうしは一致する。
const HISTORY_PROBE_PATCH: &str = "01 Basses/1984 Bass+Lead.h2p";
const HISTORY_PROBE_NOTES: usize = 6;
/// state の反映を見る音色。新しい instance どうしが一致するもの。
pub(super) const SETTLE_PROBE_PATCHES: [&str; 3] = [
    "01 Basses/1984 Bass+Lead.h2p",
    "01 Basses/Abgrund.h2p",
    "02 Leads/8bit Game Hero.h2p",
];
const SETTLE_PROBE_BLOCKS: [usize; 6] = [0, 1, 2, 4, 8, 16];
/// state を流す前に回す空ブロック数（生成直後の instance との違いを作る）。
pub(super) const IDLE_BLOCKS_BEFORE_LOAD: usize = 73;
/// 反映しきったとみなす空回しブロック数（512 frames/block）。
const SETTLED_BLOCKS: usize = 8;
/// 反映しきったあとの、新しい instance へ流したときとの包絡の差の上限。
pub(super) const SETTLED_MAX_ENVELOPE_DIFF: f32 = 0.02;
pub(super) const AUDIBLE_PEAK: f32 = 1.0e-5;
const ENVELOPE_WINDOW: usize = 1024;
/// 別の音色だと言えるための、包絡の差の RMS / 基準の包絡の RMS の下限。
pub(super) const OTHER_PATCH_MIN_ENVELOPE_DIFF: f32 = 0.3;
/// 同じ音色を別経路で鳴らしたときに許す、包絡の差の RMS / 基準の包絡の RMS。
const SAME_PATCH_MAX_ENVELOPE_DIFF: f32 = 0.05;

pub(super) fn presets_dir() -> PathBuf {
    let cfg = cmrt_server_config::ServerConfig::load()
        .expect("config.toml が読めること（先に clap-mml-render-tui を一度起動する）");
    PathBuf::from(
        cfg.patch_dirs_of("TyrellN6")
            .into_iter()
            .next()
            .expect("registry の TyrellN6 DataPath が読めない（TyrellN6 をインストールすること）"),
    )
}

pub(super) fn preset(display: &str) -> Vec<u8> {
    let path = presets_dir().join(display);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn tyrelln6_plugin_path() -> String {
    cmrt_server_config::builtin_plugin_profiles()
        .remove("TyrellN6")
        .expect("組み込みプロファイル 'TyrellN6' が無い")
        .plugin_path
}

pub(super) fn tyrelln6_entry() -> PluginEntry {
    load_entry(&tyrelln6_plugin_path()).unwrap()
}

pub(super) fn tyrelln6_renderer(entry: &PluginEntry) -> RealtimeRenderer {
    RealtimeRenderer::new(&test_config_with_plugin_id(TYRELLN6_PLUGIN_ID), entry).unwrap()
}

/// C4 を約 0.5 秒押して離し、約 0.25 秒の余韻まで録る。
pub(super) fn render_c4(renderer: &mut RealtimeRenderer) -> Vec<f32> {
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

pub(super) fn rms(samples: &[f32]) -> f32 {
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len().max(1) as f64).sqrt() as f32
}

fn envelope(samples: &[f32]) -> Vec<f32> {
    samples.chunks(ENVELOPE_WINDOW).map(rms).collect()
}

/// 包絡どうしの `rms(actual - reference) / rms(reference)`。
pub(super) fn envelope_diff(actual: &[f32], reference: &[f32]) -> f32 {
    assert_eq!(actual.len(), reference.len());
    let (actual, reference) = (envelope(actual), envelope(reference));
    let diff: Vec<f32> = actual.iter().zip(&reference).map(|(a, r)| a - r).collect();
    rms(&diff) / rms(&reference).max(f32::MIN_POSITIVE)
}

/// `#cm=<module>` の下の `key=value` 行を `<module>/key=value` の集合にする。
/// 末尾のバイナリ風ブロック（`=` を含まない行から始まり checksum 行で終わる）は数えない。
pub(super) fn module_params(bytes: &[u8]) -> BTreeSet<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut module = None;
    let mut params = BTreeSet::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("#cm=") {
            module = Some(name.to_string());
            continue;
        }
        let Some(module) = &module else { continue };
        match line.split_once('=') {
            Some((key, value)) if !key.starts_with('#') => {
                params.insert(format!("{module}/{key}={value}"));
            }
            Some(_) => {}
            None => break,
        }
    }
    params
}

/// `history` の音色で先に `notes` 回 C4 を鳴らし、空回し `idle` ブロックのあとに音色を流し、
/// `settle` ブロック回してから C4 を録る。
pub(super) struct LoadRun<'a> {
    history: Option<&'a [u8]>,
    notes: usize,
    idle: usize,
    settle: usize,
}

impl LoadRun<'_> {
    pub(super) const FRESH: LoadRun<'static> = LoadRun {
        history: None,
        notes: 0,
        idle: 0,
        settle: 0,
    };

    /// 録った音と、録ったあとの state。
    pub(super) fn render(&self, entry: &PluginEntry, patch: &[u8]) -> (Vec<f32>, Vec<u8>) {
        let mut renderer = tyrelln6_renderer(entry);
        if let Some(history) = self.history {
            load_plugin_state(renderer.plugin_instance_mut(), history).unwrap();
            for _ in 0..self.notes {
                render_c4(&mut renderer);
            }
            renderer.reset();
        }
        for _ in 0..self.idle {
            renderer.render_live_chunk(&[]).unwrap();
        }
        load_plugin_state(renderer.plugin_instance_mut(), patch).unwrap();
        for _ in 0..self.settle {
            renderer.render_live_chunk(&[]).unwrap();
        }
        let samples = render_c4(&mut renderer);
        let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
        (samples, state)
    }
}

fn escaped_head(bytes: &[u8], len: usize) -> String {
    bytes[..bytes.len().min(len)]
        .iter()
        .flat_map(|byte| std::ascii::escape_default(*byte))
        .map(char::from)
        .collect()
}

#[test]
#[ignore = "実 TyrellN6 CLAP が要る"]
fn tyrelln6_descriptor_and_capabilities() {
    let report = probe_plugin_capabilities(&tyrelln6_plugin_path(), None).unwrap();
    eprintln!("tyrelln6 descriptors={:#?}", report.descriptors);
    eprintln!(
        "tyrelln6 selected={} main_output_channels={} audio_output_ports={} input_note_dialects={:?} factories={:?} extensions={:?} param_count={} rejected={:?}",
        report.selected.id,
        report.main_output_channels,
        report.audio_output_ports,
        report.input_note_dialects,
        report.factories,
        report.extensions,
        report.param_count,
        report.rejected,
    );

    assert_eq!(report.descriptors.len(), 1);
    assert_eq!(report.selected.id, TYRELLN6_PLUGIN_ID);
    assert_eq!(report.rejected, None);
    assert_eq!(report.main_output_channels, 2);
    assert!(report.extensions.contains(&"clap.state".to_string()));
}

/// 生成直後の state は `.h2p` と同じ `#AM=TyrellN6` のテキストで、`/*@Meta` は付かない。
#[test]
#[ignore = "実 TyrellN6 CLAP が要る"]
fn fresh_state_is_the_same_text_format_as_h2p() {
    let entry = tyrelln6_entry();
    let mut renderer = tyrelln6_renderer(&entry);
    let state = save_plugin_state(renderer.plugin_instance_mut()).unwrap();
    let nul_count = state.iter().filter(|byte| **byte == 0).count();
    eprintln!(
        "tyrelln6 fresh state len={} nul_count={} head={}",
        state.len(),
        nul_count,
        escaped_head(&state, 600)
    );
    eprintln!(
        "tyrelln6 fresh state tail={}",
        escaped_head(&state[state.len().saturating_sub(120)..], 120)
    );

    let text = String::from_utf8_lossy(&state);
    assert!(text.contains("#AM=TyrellN6"));
    assert!(!module_params(&state).is_empty());
}

/// `.h2p` のバイト列を包み直さずに state として流すと、その音色の param が state に載り、
/// 生成直後とは違う音で鳴る。流した直後（空回しなし）の 1 音目も、空回しを 1 ブロック
/// 挟んだ 1 音目と同じに鳴るか（state の反映が遅れないか）も測る。
#[test]
#[ignore = "実 TyrellN6 CLAP と音色置き場が要る"]
fn raw_h2p_bytes_load_as_state_and_change_the_sound() {
    let entry = tyrelln6_entry();
    let file = preset(PATCH_A);
    let file_params = module_params(&file);

    let mut fresh = tyrelln6_renderer(&entry);
    let fresh_state = save_plugin_state(fresh.plugin_instance_mut()).unwrap();
    let init_sound = render_c4(&mut fresh);
    drop(fresh);

    let mut loaded = tyrelln6_renderer(&entry);
    load_plugin_state(loaded.plugin_instance_mut(), &file).unwrap();
    let first_note = render_c4(&mut loaded);
    let loaded_state = save_plugin_state(loaded.plugin_instance_mut()).unwrap();
    drop(loaded);

    let mut settled = tyrelln6_renderer(&entry);
    load_plugin_state(settled.plugin_instance_mut(), &file).unwrap();
    settled.render_live_chunk(&[]).unwrap();
    let settled_note = render_c4(&mut settled);
    drop(settled);

    let loaded_params = module_params(&loaded_state);
    let fresh_params = module_params(&fresh_state);
    let in_loaded = file_params.intersection(&loaded_params).count();
    let in_fresh = file_params.intersection(&fresh_params).count();
    let missing: Vec<&String> = file_params.difference(&loaded_params).collect();
    eprintln!(
        "tyrelln6 {PATCH_A}: file_params={} in_loaded_state={in_loaded} in_fresh_state={in_fresh} loaded_state_len={} missing_from_loaded={missing:?}",
        file_params.len(),
        loaded_state.len(),
    );

    let changed = envelope_diff(&first_note, &init_sound);
    let first_vs_settled = envelope_diff(&first_note, &settled_note);
    eprintln!(
        "tyrelln6 rms(init)={:.5} rms(first note, no settle)={:.5} rms(after 1 empty block)={:.5} diff(first, init)={changed:.5} diff(first, settled)={first_vs_settled:.5}",
        rms(&init_sound),
        rms(&first_note),
        rms(&settled_note),
    );

    assert!(in_loaded > in_fresh, "state に音色の param が載っていない");
    assert!(peak(&init_sound) > AUDIBLE_PEAK, "生成直後が無音");
    assert!(
        peak(&first_note) > AUDIBLE_PEAK,
        "ロード直後の 1 音目が無音"
    );
    assert!(
        changed >= OTHER_PATCH_MIN_ENVELOPE_DIFF,
        "ロードしても生成直後と音が変わらない（{changed}）"
    );
}

/// A→B と切り替えた B の state は、B→B（同じ回数だけ鳴らしたあと B を流し直す）と
/// バイト単位で一致する。A の param は state に残らない。
///
/// 音は新しい instance の B 単独と包絡が一致しない。B→B でも同じ程度にずれるので、
/// 打鍵の履歴による揺らぎで、A が残っているのではない（[`the_same_patch_does_not_repeat_after_played_notes`]）。
#[test]
#[ignore = "実 TyrellN6 CLAP と音色置き場が要る"]
fn raw_h2p_switch_from_a_to_b_leaves_no_state_of_a() {
    let entry = tyrelln6_entry();
    let (a, b) = (preset(PATCH_A), preset(PATCH_B));
    let after = |history| LoadRun {
        history: Some(history),
        notes: 1,
        idle: 0,
        settle: SETTLED_BLOCKS,
    };

    let (b_alone, _) = LoadRun::FRESH.render(&entry, &b);
    let (a_sound, _) = LoadRun::FRESH.render(&entry, &a);
    let (b_after_a, b_after_a_state) = after(&a).render(&entry, &b);
    let (b_after_b, b_after_b_state) = after(&b).render(&entry, &b);

    let same = envelope_diff(&b_after_a, &b_alone);
    let history = envelope_diff(&b_after_b, &b_alone);
    let other = envelope_diff(&a_sound, &b_alone);
    eprintln!(
        "tyrelln6 {PATCH_A} -> {PATCH_B} settle={SETTLED_BLOCKS}: state(A→B)==state(B→B)={} diff(A→B, B alone)={same:.5} diff(B→B, B alone)={history:.5} diff(A, B alone)={other:.5}",
        b_after_a_state == b_after_b_state,
    );

    assert!(peak(&b_after_a) > AUDIBLE_PEAK, "A→B の B が無音");
    assert!(
        b_after_a_state == b_after_b_state,
        "A→B の state が B→B と違う（A の値が残っている）"
    );
    assert!(
        other >= OTHER_PATCH_MIN_ENVELOPE_DIFF,
        "A と B の出音が近すぎる（{other}）"
    );
    assert!(same < other, "A→B の B が A に近い（{same} >= {other}）");
}

/// 音を鳴らしたあとの instance では、同じ音色を流し直しても新しい instance と同じ音にならない。
/// 鳴らした回数によってずれ方が変わる。
#[test]
#[ignore = "実 TyrellN6 CLAP と音色置き場が要る"]
fn the_same_patch_does_not_repeat_after_played_notes() {
    let entry = tyrelln6_entry();
    let b = preset(HISTORY_PROBE_PATCH);
    let (alone, _) = LoadRun::FRESH.render(&entry, &b);
    let (alone_again, _) = LoadRun::FRESH.render(&entry, &b);

    let mut diffs = Vec::new();
    for notes in 1..=HISTORY_PROBE_NOTES {
        let run = LoadRun {
            history: Some(&b),
            notes,
            idle: 0,
            settle: SETTLED_BLOCKS,
        };
        diffs.push(envelope_diff(&run.render(&entry, &b).0, &alone));
    }
    let fresh = envelope_diff(&alone_again, &alone);
    eprintln!(
        "tyrelln6 {HISTORY_PROBE_PATCH}: diff(fresh, fresh)={fresh:.5} diff(B after n notes of B, fresh B) n=1..={HISTORY_PROBE_NOTES}: {diffs:.5?}"
    );

    assert!(fresh <= SAME_PATCH_MAX_ENVELOPE_DIFF);
    assert!(diffs
        .iter()
        .all(|diff| *diff > SAME_PATCH_MAX_ENVELOPE_DIFF));
}

/// 音を処理したあとの instance へ流した state は、その場では反映しきらず、数ブロックかけて
/// 新しい値へ寄っていく。空回しなしの 1 音目は、前の音色から寄っていく途中の音になる。
#[test]
#[ignore = "実 TyrellN6 CLAP と音色置き場が要る"]
fn state_loaded_after_processing_settles_over_blocks() {
    let entry = tyrelln6_entry();
    let mut summary = Vec::new();
    for patch in SETTLE_PROBE_PATCHES {
        let bytes = preset(patch);
        let (alone, _) = LoadRun::FRESH.render(&entry, &bytes);
        let diffs: Vec<(usize, f32)> = SETTLE_PROBE_BLOCKS
            .iter()
            .map(|settle| {
                let run = LoadRun {
                    history: None,
                    notes: 0,
                    idle: IDLE_BLOCKS_BEFORE_LOAD,
                    settle: *settle,
                };
                (
                    *settle,
                    envelope_diff(&run.render(&entry, &bytes).0, &alone),
                )
            })
            .collect();
        eprintln!("tyrelln6 settle {patch}: (blocks, diff vs fresh load) {diffs:.4?}");
        summary.push(diffs);
    }

    for diffs in &summary {
        let at = |blocks| diffs.iter().find(|(b, _)| *b == blocks).unwrap().1;
        assert!(
            at(0) > SAME_PATCH_MAX_ENVELOPE_DIFF,
            "空回しなしで反映しきっている"
        );
        assert!(at(SETTLED_BLOCKS) <= SETTLED_MAX_ENVELOPE_DIFF);
    }
}
