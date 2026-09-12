//! **小節境界に音の途切れが無いこと**を、出力サンプル列で固定する。
//!
//! 「小節ごとにぶつ切り」の正体は `refresh_buffer()` の `stop_all()` で、小節境界で
//! 全 voice が殺され 100ms 強の無音が空いていた。ジッタ（「モタる」）は別物で、
//! TUI 側 `tests/jitter.rs` が判定する。
//!
//! [`super::sustain`] は「スロットを差し替えても voice が生き残る」という**部品**を見る。
//! ここは **DAW の演奏ループと同じ順番**（先読み → 小節境界で note on → 次の小節を先読み）
//! を丸ごと回して、**出てきたサンプル列に切れ目が無い**ことを見る。部品が正しくても
//! 呼ぶ順番が違えば途切れるので、両方要る。
//!
//! # 判定の作り方
//! 小節ごとに違う音源を鳴らし、**期待値を全フレーム・両チャンネルで厳密比較**する。
//!
//! - 小節 1 … ランプ（フレーム番号に比例）。**再生位置が続いているか**が値で判る
//! - 小節 2 / 3 … それぞれ別の定数。境界から先に「足された」ことが判る
//!
//! 「無音でないこと」だけを見るテストにはしない。前の小節の音が**別の位置から**
//! 鳴り直しても、値が 0 でなければ通ってしまうため。

use clack_host::prelude::*;

use crate::slots::slot_patch_state;
use crate::tests::harness::*;
use crate::SLOT_COUNT;

/// 1 小節のブロック数。実際の DAW は 1 小節 2.4 秒（約 225 ブロック）だが、
/// 判定に要るのは「境界をまたいで前の小節の音が続くか」だけなので最小構成でよい。
const MEASURE_BLOCKS: usize = 2;
/// 1 小節のフレーム数。
const MEASURE_FRAMES: usize = MEASURE_BLOCKS * BLOCK_FRAMES;
/// キャッシュ WAV の長さ。実際の DAW と同じく**小節長より長く**して、
/// 余韻が次の小節へはみ出す状況を作る。ここが小節長ちょうどだと、
/// `stop_all()` を戻しても差が出ない（どのみち鳴り終わっている）。
const WAV_FRAMES: usize = 4 * MEASURE_FRAMES;

/// 小節 2 の音源の値（L, R）。小節 1 のランプと足し合わせても元の値に戻らない定数を選ぶ。
const MEASURE2: (f32, f32) = (0.5, -0.25);
/// 小節 3 の音源の値（L, R）。
const MEASURE3: (f32, f32) = (0.125, 0.0625);

/// 小節 index `N` の note number。**TUI 側 `live_cache/cues.rs` と同じ規則**
/// （`60 + (N % SLOT_COUNT)`）。ここを勝手に変えると、鳴ってはいるが 1 小節ずれる。
fn note_of_measure(measure_index: usize) -> u8 {
    60 + (measure_index % SLOT_COUNT) as u8
}

/// 3 小節を通しで鳴らし、**境界を含む全フレーム**が期待どおりであること。
///
/// 期待値は「小節 1 のランプ ＋ 境界から先に足される定数」。前の小節の余韻が
/// 境界で消えれば（`stop_all()` が戻れば）ランプの成分が落ちて赤くなる。
#[test]
fn measure_boundaries_leave_no_gap_in_the_output() {
    let measure1 = write_ramp_wav("cmrt_cache_player_boundary_m1.wav", WAV_FRAMES);
    let measure2 = write_test_wav(
        "cmrt_cache_player_boundary_m2.wav",
        WAV_FRAMES,
        MEASURE2.0,
        MEASURE2.1,
    );
    let measure3 = write_test_wav(
        "cmrt_cache_player_boundary_m3.wav",
        WAV_FRAMES,
        MEASURE3.0,
        MEASURE3.1,
    );

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);
    // 演奏開始: 小節 1（index 0）をスロット 0 へ。
    load_state(
        &mut plugin,
        &slot_patch_state(0, measure1.to_str().unwrap()),
    );

    let mut processor = start_processing(&mut plugin);
    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs = AudioPorts::with_capacity(0, 0);
    let mut outputs = AudioPorts::with_capacity(2, 1);
    let mut rendered = [Vec::new(), Vec::new()];

    for block in 0..3 * MEASURE_BLOCKS {
        let measure_index = block / MEASURE_BLOCKS;
        let at_boundary = block % MEASURE_BLOCKS == 0;
        let note = [note_of_measure(measure_index)];

        let output = process_block(
            &mut processor,
            &mut inputs,
            &mut outputs,
            &mut output_events,
            if at_boundary {
                TestNotes::Midi(&note)
            } else {
                TestNotes::None
            },
        );
        rendered[0].extend_from_slice(&output[0]);
        rendered[1].extend_from_slice(&output[1]);

        // DAW のループと同じ並び: 境界では note on を出したあとに次の小節を先読みする。
        // 先に先読みを出すと、その state load がそのまま小節の頭の遅れになる。
        if at_boundary {
            let next = measure_index + 1;
            let path = match next {
                1 => Some(&measure2),
                2 => Some(&measure3),
                _ => None,
            };
            if let Some(path) = path {
                load_state(
                    &mut plugin,
                    &slot_patch_state(next % SLOT_COUNT, path.to_str().unwrap()),
                );
            }
        }
    }

    assert_measures_are_seamless(&rendered);

    plugin.deactivate(processor.stop_processing());
}

/// **先読みが外れた小節**（境界に着いてから state load を出す形）でも、
/// 前の小節の余韻が切れないこと。
///
/// 演奏中に AB リピートや小節数が変わると `preload=miss` になり、この経路を通る。
/// スロットの差し替えと note on が**同じブロック**に来るのが上のテストとの違い。
#[test]
fn a_late_load_at_the_boundary_still_keeps_the_previous_measure_sounding() {
    let measure1 = write_ramp_wav("cmrt_cache_player_late_load_m1.wav", WAV_FRAMES);
    let measure2 = write_test_wav(
        "cmrt_cache_player_late_load_m2.wav",
        WAV_FRAMES,
        MEASURE2.0,
        MEASURE2.1,
    );

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);
    load_state(
        &mut plugin,
        &slot_patch_state(0, measure1.to_str().unwrap()),
    );

    let mut processor = start_processing(&mut plugin);
    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs = AudioPorts::with_capacity(0, 0);
    let mut outputs = AudioPorts::with_capacity(2, 1);
    let mut rendered = [Vec::new(), Vec::new()];

    for block in 0..2 * MEASURE_BLOCKS {
        let measure_index = block / MEASURE_BLOCKS;
        let at_boundary = block % MEASURE_BLOCKS == 0;
        // 先読みしていないので、境界に着いてから載せる（load は main thread なので
        // 実際にはここで 100〜600ms 掛かる）。
        if at_boundary && measure_index == 1 {
            load_state(
                &mut plugin,
                &slot_patch_state(1, measure2.to_str().unwrap()),
            );
        }
        let note = [note_of_measure(measure_index)];
        let output = process_block(
            &mut processor,
            &mut inputs,
            &mut outputs,
            &mut output_events,
            if at_boundary {
                TestNotes::Midi(&note)
            } else {
                TestNotes::None
            },
        );
        rendered[0].extend_from_slice(&output[0]);
        rendered[1].extend_from_slice(&output[1]);
    }

    assert_measures_are_seamless(&rendered);

    plugin.deactivate(processor.stop_processing());
}

/// 小節 1 のランプが最後まで途切れず、境界から先に小節 2 / 3 の定数が足されていること。
///
/// `rendered` は 1 小節ぶんでも 3 小節ぶんでもよい（長さで判断する）。
fn assert_measures_are_seamless(rendered: &[Vec<f32>; 2]) {
    let frames = rendered[0].len();
    assert_eq!(rendered[1].len(), frames, "L と R の長さが違う");
    assert!(frames >= 2 * MEASURE_FRAMES, "境界を 1 つも通っていない");

    for (frame, (left, right)) in rendered[0].iter().zip(&rendered[1]).enumerate() {
        let added = added_at(frame);
        assert_eq!(
            *left,
            ramp_at(frame) + added.0,
            "L: 小節境界で音が途切れている（frame={frame}, 小節長={MEASURE_FRAMES}）"
        );
        assert_eq!(
            *right,
            -ramp_at(frame) + added.1,
            "R: 小節境界で音が途切れている（frame={frame}, 小節長={MEASURE_FRAMES}）"
        );
    }
}

/// `frame` の時点で小節 1 のランプへ足されているはずの定数（L, R）。
fn added_at(frame: usize) -> (f32, f32) {
    let mut added = (0.0, 0.0);
    if frame >= MEASURE_FRAMES {
        added.0 += MEASURE2.0;
        added.1 += MEASURE2.1;
    }
    if frame >= 2 * MEASURE_FRAMES {
        added.0 += MEASURE3.0;
        added.1 += MEASURE3.1;
    }
    added
}
