use std::sync::Arc;

use crate::buffer::CacheBuffer;
use crate::graveyard::{BufferGraveyard, SharedGraveyard, GRAVEYARD_CAPACITY};

fn buffer() -> Arc<CacheBuffer> {
    Arc::new(CacheBuffer::from_channels(vec![vec![0.0; 4]], 48_000))
}

/// graveyard へ預けたあいだは解放されず、main thread が引き取った時点で解放されること。
///
/// 「解放されていない」は `Arc::strong_count` で見る。RT スレッドで `drop` が走ったかを
/// 直接は観測できないので、**参照が残っているか**を数値で見るのがここでの判定。
#[test]
fn a_buried_arc_is_released_only_when_the_main_thread_reclaims_it() {
    let shared = SharedGraveyard::default();
    let mut pending = BufferGraveyard::new();
    let buffer = buffer();

    // RT スレッド側: drop せずに預ける。
    pending.bury(Arc::clone(&buffer));
    assert_eq!(Arc::strong_count(&buffer), 2, "預けた時点で解放されている");

    shared.try_collect(&mut pending);
    assert!(pending.is_empty(), "共有側へ move されていない");
    assert_eq!(shared.pending_count(), 1);
    assert_eq!(Arc::strong_count(&buffer), 2, "移送で解放されている");

    // main thread 側: ここで初めて解放される。
    shared.reclaim();
    assert_eq!(shared.pending_count(), 0);
    assert_eq!(
        Arc::strong_count(&buffer),
        1,
        "引き取っても解放されていない"
    );
}

/// 共有側が満杯なら手元に残り、次の `try_collect` で移せること。
/// 移せなかったぶんを RT スレッドで捨てないのが要点。
#[test]
fn what_does_not_fit_stays_pending_instead_of_being_dropped() {
    let shared = SharedGraveyard::default();
    let mut pending = BufferGraveyard::new();

    for _ in 0..GRAVEYARD_CAPACITY {
        pending.bury(buffer());
    }
    shared.try_collect(&mut pending);
    assert_eq!(shared.pending_count(), GRAVEYARD_CAPACITY);

    let extra = buffer();
    pending.bury(Arc::clone(&extra));
    shared.try_collect(&mut pending);
    assert_eq!(pending.len(), 1, "満杯の共有側へ押し込めてしまっている");
    assert_eq!(Arc::strong_count(&extra), 2, "移せなかったぶんが捨てられた");

    shared.reclaim();
    shared.try_collect(&mut pending);
    assert!(pending.is_empty());
    assert_eq!(shared.pending_count(), 1);
}

/// 手元も満杯になったときだけ、その場で解放して数を数えること（最後の砦）。
#[test]
fn burying_beyond_the_capacity_is_counted_as_an_overflow() {
    let mut pending = BufferGraveyard::new();
    for _ in 0..GRAVEYARD_CAPACITY {
        pending.bury(buffer());
    }
    assert_eq!(pending.overflowed(), 0);

    pending.bury(buffer());
    assert_eq!(pending.len(), GRAVEYARD_CAPACITY);
    assert_eq!(pending.overflowed(), 1);
}
