//! **スロットを差し替えても鳴っている音が切れないこと。**
//!
//! ここが Stage 2 の本命。差し替えのたびに `stop_all()` を呼んでいたせいで、
//! 小節境界で全 track が一斉に無音になっていた（「ぶつ切り」の正体）。
//!
//! 判定はランプ WAV（フレーム番号がそのまま値になる）で行う。定数 WAV だと
//! 「鳴っているか」しか判らず、**再生位置が続いているか**が見えない。

use clack_host::prelude::*;

use crate::slots::slot_patch_state;
use crate::tests::harness::*;

/// 別のスロットを差し替えても、鳴っている voice が切れず、再生位置も続くこと。
///
/// 先読み（小節 N を鳴らしながら小節 N+1 を載せる）でこれが起きる。
#[test]
fn a_sounding_voice_survives_the_replacement_of_another_slot() {
    let sounding = write_ramp_wav("cmrt_cache_player_sustain_ramp.wav", 128);
    let loaded_later = write_test_wav("cmrt_cache_player_sustain_next.wav", 128, 0.75, -0.5);

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);
    load_state(
        &mut plugin,
        &slot_patch_state(0, sounding.to_str().unwrap()),
    );

    let mut processor = start_processing(&mut plugin);
    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs = AudioPorts::with_capacity(0, 0);
    let mut outputs = AudioPorts::with_capacity(2, 1);

    let first = process_block(
        &mut processor,
        &mut inputs,
        &mut outputs,
        &mut output_events,
        TestNotes::Midi(&[60]),
    );
    assert_eq!(first[0][1], ramp_at(1), "スロット 0 が鳴っていない");

    // 鳴っている最中に「次の小節」を別スロットへ載せる。
    load_state(
        &mut plugin,
        &slot_patch_state(1, loaded_later.to_str().unwrap()),
    );

    let second = process_block(
        &mut processor,
        &mut inputs,
        &mut outputs,
        &mut output_events,
        TestNotes::None,
    );
    // ブロック境界の 1 サンプル目。0 に落ちていたらここで切れている。
    assert_ne!(second[0][0], 0.0, "差し替えで音が切れている");
    assert_ramp_continues(&second, BLOCK_FRAMES, (0.0, 0.0), "再生位置が続いていない");

    plugin.deactivate(processor.stop_processing());
}

/// **自分自身のスロットを差し替えても**、鳴っている voice は元の音源を鳴らし続けること。
///
/// 判断3（voice は `Arc` を自分で握り、スロット index を参照しない）の中心。
/// あわせて、差し替えが効いていること（次の note on は新しい音源で鳴る）まで見る。
/// これが無いと「state load を無視する」実装でも前半の assert が通ってしまう。
#[test]
fn a_sounding_voice_keeps_its_own_buffer_when_that_slot_is_replaced() {
    let sounding = write_ramp_wav("cmrt_cache_player_replace_ramp.wav", 128);
    let replacement = write_test_wav("cmrt_cache_player_replace_next.wav", 128, 0.75, -0.5);

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);
    load_state(
        &mut plugin,
        &slot_patch_state(0, sounding.to_str().unwrap()),
    );

    let mut processor = start_processing(&mut plugin);
    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs = AudioPorts::with_capacity(0, 0);
    let mut outputs = AudioPorts::with_capacity(2, 1);

    process_block(
        &mut processor,
        &mut inputs,
        &mut outputs,
        &mut output_events,
        TestNotes::Midi(&[60]),
    );

    // 同じスロット 0 を別の音源で上書きする。
    load_state(
        &mut plugin,
        &slot_patch_state(0, replacement.to_str().unwrap()),
    );

    let second = process_block(
        &mut processor,
        &mut inputs,
        &mut outputs,
        &mut output_events,
        TestNotes::None,
    );
    assert_ramp_continues(
        &second,
        BLOCK_FRAMES,
        (0.0, 0.0),
        "鳴っている voice の音源が入れ替わっている",
    );

    // 次の note on は新しい音源で鳴る（＝差し替え自体は効いている）。
    // 前の voice はまだ鳴っているので、その和になる。
    let third = process_block(
        &mut processor,
        &mut inputs,
        &mut outputs,
        &mut output_events,
        TestNotes::Midi(&[60]),
    );
    assert_ramp_continues(
        &third,
        2 * BLOCK_FRAMES,
        (0.75, -0.5),
        "差し替えた音源が新しい note で鳴っていない",
    );

    plugin.deactivate(processor.stop_processing());
}

/// 出力 1 ブロックが「ランプ WAV の `from` フレーム目からの続き」に
/// `added` を足したものになっていることを、全フレーム・両チャンネルで確かめる。
///
/// 1 サンプルだけ見ると、たまたま一致する値で通ってしまう。
fn assert_ramp_continues(output: &[Vec<f32>; 2], from: usize, added: (f32, f32), context: &str) {
    for (frame, left) in output[0].iter().enumerate().take(BLOCK_FRAMES) {
        assert_eq!(
            *left,
            ramp_at(from + frame) + added.0,
            "L: {context}（frame={frame}）"
        );
    }
    for (frame, right) in output[1].iter().enumerate().take(BLOCK_FRAMES) {
        assert_eq!(
            *right,
            -ramp_at(from + frame) + added.1,
            "R: {context}（frame={frame}）"
        );
    }
}
