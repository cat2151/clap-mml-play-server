//! note number でスロットを選び分けること。

use clack_host::prelude::*;

use crate::slots::slot_patch_state;
use crate::tests::harness::*;

/// スロットの本命。**別々の WAV を 2 つのスロットへ載せ、2 つの note number で
/// 同時に鳴らすと、出力に両方の成分が混ざる**こと。
///
/// L/R に別々の定数を書いた WAV を使うので、「両方鳴ったか」「どちらか片方だけか」
/// 「チャンネルが入れ替わっていないか」が 1 サンプルの値で判る。
#[test]
fn two_slots_hold_different_wavs_and_sound_together() {
    // 和が元のどちらとも一致しない値を選ぶ（片方だけ鳴っても等しくならない）。
    let slot0 = write_test_wav("cmrt_cache_player_slot0.wav", 64, 0.5, -0.25);
    let slot1 = write_test_wav("cmrt_cache_player_slot1.wav", 64, 0.125, 0.0625);

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);
    load_state(&mut plugin, &slot_patch_state(0, slot0.to_str().unwrap()));
    load_state(&mut plugin, &slot_patch_state(1, slot1.to_str().unwrap()));

    let mut processor = start_processing(&mut plugin);
    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs_descriptors = AudioPorts::with_capacity(0, 0);
    let mut outputs_descriptors = AudioPorts::with_capacity(2, 1);

    // note 60 -> スロット 0、note 61 -> スロット 1（`slots::slot_for_note`）。
    let mixed = process_block(
        &mut processor,
        &mut inputs_descriptors,
        &mut outputs_descriptors,
        &mut output_events,
        TestNotes::Midi(&[60, 61]),
    );

    assert_eq!(mixed[0][0], 0.5 + 0.125, "L に両方のスロットが乗っていない");
    assert_eq!(
        mixed[1][0],
        -0.25 + 0.0625,
        "R に両方のスロットが乗っていない"
    );

    plugin.deactivate(processor.stop_processing());
}

/// スロット 1 だけを鳴らしたときに、スロット 0 の音が混ざらないこと。
///
/// 上のテストは「足し合わされたか」しか見ないので、note number を無視して
/// 全スロットを鳴らす実装でも通ってしまう。ここがその裏取り。
#[test]
fn a_note_only_sounds_its_own_slot() {
    let slot0 = write_test_wav("cmrt_cache_player_only_slot0.wav", 64, 0.5, -0.25);
    let slot1 = write_test_wav("cmrt_cache_player_only_slot1.wav", 64, 0.125, 0.0625);

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);
    load_state(&mut plugin, &slot_patch_state(0, slot0.to_str().unwrap()));
    load_state(&mut plugin, &slot_patch_state(1, slot1.to_str().unwrap()));

    let mut processor = start_processing(&mut plugin);
    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs_descriptors = AudioPorts::with_capacity(0, 0);
    let mut outputs_descriptors = AudioPorts::with_capacity(2, 1);

    let only_slot1 = process_block(
        &mut processor,
        &mut inputs_descriptors,
        &mut outputs_descriptors,
        &mut output_events,
        TestNotes::Midi(&[61]),
    );

    assert_eq!(only_slot1[0][0], 0.125, "スロット 1 以外の音が混ざっている");
    assert_eq!(only_slot1[1][0], 0.0625);

    plugin.deactivate(processor.stop_processing());
}
