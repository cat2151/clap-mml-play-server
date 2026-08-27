//! bank への振り分けの検証。
//!
//! worker スレッドそのものは実 CLAP インスタンスが要るので、ここで確かめられるのは
//! 「どの instance の仕事がどちらの bank の何番へ行くか」まで。thread が本当に
//! 分かれているかは実サーバーを起こす統合テスト
//! （`clap-mml-render-tui` の `realtime-play/src/live_ipc/tests.rs`）が見る。

use super::*;

fn note(key: u8) -> LiveMidiEvent {
    LiveMidiEvent {
        offset_frames: 0,
        message: [0x90, key, 100],
    }
}

fn keys(events: &[LiveMidiEvent]) -> Vec<u8> {
    events.iter().map(|event| event.message[1]).collect()
}

/// global instance index が、正しい bank の正しい local index へ割れること。
#[test]
fn events_go_to_the_bank_that_owns_the_instance() {
    let layout = BankLayout::split_any(4);
    let jobs = split_events_by_bank(
        &layout,
        vec![
            Some(vec![note(60)]),
            Some(vec![note(61)]),
            Some(vec![note(62)]),
            Some(vec![note(63)]),
        ],
    );

    assert_eq!(jobs[0].len(), 2);
    assert_eq!(jobs[1].len(), 2);
    assert_eq!(jobs[0][0].local_index, 0);
    assert_eq!(keys(&jobs[0][0].events), vec![60]);
    assert_eq!(jobs[0][1].local_index, 1);
    assert_eq!(keys(&jobs[0][1].events), vec![61]);
    // 後半は bank 1 の 0 番から数え直す。
    assert_eq!(jobs[1][0].local_index, 0);
    assert_eq!(keys(&jobs[1][0].events), vec![62]);
    assert_eq!(jobs[1][1].local_index, 1);
    assert_eq!(keys(&jobs[1][1].events), vec![63]);
}

/// 非 active な instance は render を要求しない（`None` は落ちる）。
#[test]
fn inactive_instances_are_not_requested() {
    let layout = BankLayout::split_any(4);
    let jobs = split_events_by_bank(&layout, vec![None, Some(vec![note(61)]), None, None]);

    assert_eq!(jobs[0].len(), 1);
    assert_eq!(jobs[0][0].local_index, 1);
    assert!(jobs[1].is_empty(), "bank 1 へは要求が飛ばない");
}

/// bank の中では local index 昇順のまま並ぶこと。mix 順が毎ブロック同じであるための前提。
#[test]
fn instances_stay_in_local_index_order_within_a_bank() {
    let layout = BankLayout::split_any(16);
    let jobs = split_events_by_bank(
        &layout,
        (0..16).map(|key| Some(vec![note(key as u8)])).collect(),
    );

    let bank0: Vec<usize> = jobs[0].iter().map(|job| job.local_index).collect();
    let bank1: Vec<usize> = jobs[1].iter().map(|job| job.local_index).collect();
    assert_eq!(bank0, (0..8).collect::<Vec<_>>());
    assert_eq!(bank1, (0..8).collect::<Vec<_>>());
}

/// instance 1 個の構成では bank 1 が空のまま。worker は立つが仕事は来ない。
#[test]
fn a_single_instance_server_only_uses_bank_zero() {
    let layout = BankLayout::split_any(1);
    let jobs = split_events_by_bank(&layout, vec![Some(vec![note(60)])]);

    assert_eq!(jobs[0].len(), 1);
    assert!(jobs[1].is_empty());
}
