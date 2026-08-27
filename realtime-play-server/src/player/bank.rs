//! live instance を 2 つの bank へ割る境界。
//!
//! grid sequencer は N トラックを 2 bank（= 2N instance）へ割り当て、鳴っている
//! bank の裏でもう一方へ次の音色を先読みする。**global instance ID の前半が bank 0、
//! 後半が bank 1** で、クライアント側の割り当て（`grid-sequencer` の
//! `state/cycle.rs` の `instance_id()` / `standby_instance_id()`）と同じ規則。
//!
//! ここは「どの instance がどの bank の何番目か」だけを持つ。renderer も
//! patch load も持たない（それらは worker 側の仕事）。

use anyhow::Result;
use cmrt_realtime_ipc::InstanceId;

/// bank の数。増やす計画は無い（先読みに要るのは「鳴っている側」と「その裏」だけ）。
pub(super) const BANK_COUNT: usize = 2;

/// bank 内での位置。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BankSlot {
    pub(super) bank: usize,
    pub(super) local_index: usize,
}

/// live instance 全体を bank へ割る規則。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BankLayout {
    bank_sizes: [usize; BANK_COUNT],
}

impl BankLayout {
    /// 設定された live instance 数から作る。**先読みが成り立つ構成だけを受ける。**
    ///
    /// 奇数だと 2 bank へ均等に割れない。`CMRT_LIVE_INSTANCE_COUNT=1` のような
    /// 設定は先読みそのものが成り立たないので、ここで断る。
    pub(super) fn new(live_instance_count: usize) -> Result<Self> {
        if live_instance_count == 0 || !live_instance_count.is_multiple_of(BANK_COUNT) {
            anyhow::bail!(
                "live instance count {live_instance_count} cannot be split into {BANK_COUNT} banks"
            );
        }
        Ok(Self::split_any(live_instance_count))
    }

    /// どんな instance 数でも 2 bank へ割る。端数は bank 0 に付ける。
    ///
    /// **worker の分割はこちらを使う。** サーバーは先読みが成り立たない構成
    /// （`CMRT_LIVE_INSTANCE_COUNT=1`）でも起動しなければならないので、
    /// [`Self::new`] の「断る」判断を worker の生成へ持ち込めない。
    /// 偶数なら [`Self::new`] と割り方は完全に同じで、`instance_id` →
    /// bank の対応も 1 つしか無い（食い違わせないために構築だけを分けている）。
    pub(super) fn split_any(live_instance_count: usize) -> Self {
        let first = live_instance_count.div_ceil(BANK_COUNT);
        Self {
            bank_sizes: [first, live_instance_count - first],
        }
    }

    pub(super) fn instance_count(&self) -> usize {
        self.bank_sizes.iter().sum()
    }

    /// bank が持つ instance の数。
    pub(super) fn bank_size(&self, bank: usize) -> usize {
        self.bank_sizes[bank]
    }

    /// global instance ID を bank と bank 内 index へ分ける。
    pub(super) fn slot_of(&self, instance_id: InstanceId) -> Result<BankSlot> {
        let index = usize::from(instance_id);
        if index >= self.instance_count() {
            anyhow::bail!(
                "instance {instance_id} is outside configured live range 0..{}",
                self.instance_count()
            );
        }
        Ok(self.slot_of_index(index))
    }

    /// 範囲内と分かっている index を bank 内の位置へ直す。
    ///
    /// 範囲検査は IPC の入口（`validate_live_instance_id`）で済んでいるので、
    /// worker 側の経路はこちらを使う。
    pub(super) fn slot_of_index(&self, index: usize) -> BankSlot {
        if index < self.bank_sizes[0] {
            BankSlot {
                bank: 0,
                local_index: index,
            }
        } else {
            BankSlot {
                bank: 1,
                local_index: index - self.bank_sizes[0],
            }
        }
    }

    /// bank 内の位置を global instance index へ戻す。
    pub(super) fn global_index(&self, slot: BankSlot) -> usize {
        if slot.bank == 0 {
            slot.local_index
        } else {
            self.bank_sizes[0] + slot.local_index
        }
    }
}

#[cfg(test)]
mod tests;
