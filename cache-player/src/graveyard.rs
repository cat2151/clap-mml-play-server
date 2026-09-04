//! 使い終わった [`CacheBuffer`] の `Arc` を、RT スレッドの外で解放するための返却キュー。
//!
//! # なぜ要るのか
//! voice は自分が鳴らす音源を `Arc<CacheBuffer>` で握る（[`crate::buffer::VoiceBank`]）。
//! そのおかげでスロットを差し替えても鳴っている音が切れないが、**最後の参照が
//! RT スレッドで消えると、数 MB の `Vec<Vec<f32>>` の解放が `process` の中で走る。**
//!
//! `Arc::clone` は参照カウントを 1 増やすだけなので確保にはならない。
//! **崩れうるのは解放のほうだけ。** そこだけをここで受け止める。
//!
//! # 仕組み
//! RT スレッドは要らなくなった `Arc` を drop せず、手元の固定長 [`BufferGraveyard`] へ
//! move する（確保も解放もしない）。`process` の最後に [`SharedGraveyard::try_collect`] で
//! 共有側へ move し、main thread が [`SharedGraveyard::reclaim`] で引き取って**そこで**解放する。
//!
//! main thread 側の引き取りは CLAP state の load（＝小節ごとの先読み）で回る。

use std::sync::{Arc, Mutex};

use crate::buffer::CacheBuffer;

/// 1 つの graveyard が抱えられる `Arc` の本数。
///
/// 1 ブロックで死にうるのは「差し替えたスロット `SLOT_COUNT` 本」＋「鳴り終わった voice
/// `MAX_VOICES` 本」＋「note on で潰された voice」なので、その数倍を取ってある。
/// 抱えているのはポインタだけ（音源の実体はもともと確保済み）なので、大きくしても安い。
pub const GRAVEYARD_CAPACITY: usize = 32;

/// 固定長の `Arc` 置き場。**`process` 中に確保も解放もしない。**
pub struct BufferGraveyard {
    buried: [Option<Arc<CacheBuffer>>; GRAVEYARD_CAPACITY],
    overflowed: u64,
}

impl Default for BufferGraveyard {
    fn default() -> Self {
        Self::new()
    }
}

impl BufferGraveyard {
    pub fn new() -> Self {
        Self {
            buried: std::array::from_fn(|_| None),
            overflowed: 0,
        }
    }

    /// 要らなくなった `Arc` を預かる。
    ///
    /// **満杯のときだけ、その場で解放する。** RT スレッドから見ると最後の砦なので、
    /// 起きたことが判るように [`Self::overflowed`] で数えておく。
    pub fn bury(&mut self, buffer: Arc<CacheBuffer>) {
        for grave in self.buried.iter_mut() {
            if grave.is_none() {
                *grave = Some(buffer);
                return;
            }
        }
        self.overflowed += 1;
        drop(buffer);
    }

    /// 中身を別の graveyard へ move する。ポインタの move だけで、解放は起きない。
    ///
    /// 移しきれなかったぶんは手元に残る（次の block で再試行できる）。
    pub fn move_into(&mut self, destination: &mut BufferGraveyard) {
        for grave in self.buried.iter_mut() {
            if grave.is_none() {
                continue;
            }
            let Some(free) = destination.buried.iter_mut().find(|slot| slot.is_none()) else {
                return;
            };
            *free = grave.take();
        }
    }

    /// main thread が引き取る。**返った `Vec` を drop した時点で解放される。**
    pub fn take_all(&mut self) -> Vec<Arc<CacheBuffer>> {
        self.buried.iter_mut().filter_map(Option::take).collect()
    }

    /// 預かっている本数。
    pub fn len(&self) -> usize {
        self.buried.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 満杯で受け取れず、その場で解放した回数。0 でなければ容量が足りていない。
    pub fn overflowed(&self) -> u64 {
        self.overflowed
    }
}

/// main thread と RT スレッドの受け渡し口。
#[derive(Default)]
pub struct SharedGraveyard {
    queue: Mutex<BufferGraveyard>,
}

impl SharedGraveyard {
    /// RT スレッドから呼ぶ。**ブロックしない。**
    ///
    /// ロックが取れなければ何もしない（`pending` は手元に残るので次の block で再試行される）。
    pub fn try_collect(&self, pending: &mut BufferGraveyard) {
        if pending.is_empty() {
            return;
        }
        let Ok(mut queue) = self.queue.try_lock() else {
            return;
        };
        pending.move_into(&mut queue);
    }

    /// main thread から呼ぶ。**解放はロックを離してから**行う
    /// （解放中に RT の `try_collect` を失敗させないため）。
    pub fn reclaim(&self) {
        let reclaimed = {
            let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.take_all()
        };
        drop(reclaimed);
    }

    /// テスト・診断用。預かったまま引き取られていない本数。
    pub fn pending_count(&self) -> usize {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

#[cfg(test)]
mod tests;
