use super::*;

#[test]
fn global_instance_ids_split_into_the_first_and_second_half() {
    let layout = BankLayout::new(14).unwrap();
    assert_eq!(layout.instance_count(), 14);
    let expected = [(0u8, 0usize, 0usize), (6, 0, 6), (7, 1, 0), (13, 1, 6)];
    for (instance_id, bank, local_index) in expected {
        assert_eq!(
            layout.slot_of(instance_id).unwrap(),
            BankSlot { bank, local_index },
            "instance {instance_id}"
        );
    }
}

/// 隣り合う instance ID が同じ bank にまとまり、境界がちょうど半分に来ること。
#[test]
fn every_instance_belongs_to_exactly_one_bank() {
    let layout = BankLayout::new(16).unwrap();
    let banks: Vec<usize> = (0..16u8)
        .map(|instance_id| layout.slot_of(instance_id).unwrap().bank)
        .collect();
    assert_eq!(banks, [vec![0; 8], vec![1; 8]].concat());
    let locals: Vec<usize> = (0..16u8)
        .map(|instance_id| layout.slot_of(instance_id).unwrap().local_index)
        .collect();
    assert_eq!(
        locals,
        [(0..8).collect::<Vec<_>>(), (0..8).collect()].concat()
    );
}

/// 奇数は 2 bank へ割れない。`CMRT_LIVE_INSTANCE_COUNT=1` が該当する。
#[test]
fn odd_instance_counts_are_rejected() {
    for count in [0, 1, 3, 7, 15] {
        let error = BankLayout::new(count).unwrap_err();
        assert!(
            error.to_string().contains("cannot be split"),
            "count={count} error={error}"
        );
    }
}

#[test]
fn instance_ids_past_the_configured_count_are_rejected() {
    let layout = BankLayout::new(6).unwrap();
    assert!(layout.slot_of(5).is_ok());
    let error = layout.slot_of(6).unwrap_err();
    assert!(
        error.to_string().contains("configured live range 0..6"),
        "{error}"
    );
}

/// worker の分割は先読みが成り立たない構成でも成り立たなければならない
/// （サーバーは `CMRT_LIVE_INSTANCE_COUNT=1` でも起動する）。
#[test]
fn any_instance_count_splits_without_losing_an_instance() {
    for count in [1usize, 2, 4, 6, 8, 14, 16, 32] {
        let layout = BankLayout::split_any(count);
        assert_eq!(layout.instance_count(), count, "count={count}");
        assert_eq!(
            layout.bank_size(0) + layout.bank_size(1),
            count,
            "count={count}"
        );
        // 端数は bank 0 へ付ける。bank 1 が空でも worker は立つ。
        assert!(layout.bank_size(0) >= layout.bank_size(1), "count={count}");
    }
    let single = BankLayout::split_any(1);
    assert_eq!(single.bank_size(0), 1);
    assert_eq!(single.bank_size(1), 0);
}

/// 偶数構成では「先読み用の割り方」と「worker の割り方」が完全に一致すること。
/// ここがずれると、先読み要求と実際にロードする worker が別 bank を指す。
#[test]
fn the_lenient_split_matches_the_strict_one_for_even_counts() {
    for count in [2usize, 4, 6, 8, 14, 16, 32] {
        assert_eq!(
            BankLayout::new(count).unwrap(),
            BankLayout::split_any(count),
            "count={count}"
        );
    }
}

/// global index → bank 内の位置 → global index が往復すること。
#[test]
fn slot_and_global_index_round_trip() {
    let layout = BankLayout::split_any(14);
    for index in 0..14 {
        assert_eq!(layout.global_index(layout.slot_of_index(index)), index);
    }
}
