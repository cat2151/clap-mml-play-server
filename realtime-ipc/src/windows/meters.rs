//! 共有メモリのメーター領域の読み書き。
//!
//! コマンドのリングと違い、ここはサーバーが一方的に書きクライアントが読むだけの
//! 一方通行の領域で、応答も同期も伴わない。書き手と読み手が同じ順序規約
//! （`Release` で書き `Acquire` で読む）を使うことだけが要件なので、
//! 両方の実装を1か所へ並べて置く。

use std::sync::atomic::{AtomicU32, Ordering};

use super::{protocol::SharedRing, LimiterMeter, MAX_INSTANCE_COUNT};

pub(super) fn publish_limiter_meter(ring: &SharedRing, meter: LimiterMeter) {
    ring.limiter_current_bits
        .store(meter.current_reduction_db.to_bits(), Ordering::Release);
    update_atomic_max(&ring.limiter_peak_bits, meter.peak_reduction_db);
}

/// ピークは読んだ時点で 0 へ戻す。次に読むまでの間の最大値を返すため。
pub(super) fn limiter_meter(ring: &SharedRing) -> LimiterMeter {
    LimiterMeter {
        current_reduction_db: f32::from_bits(ring.limiter_current_bits.load(Ordering::Acquire)),
        peak_reduction_db: f32::from_bits(ring.limiter_peak_bits.swap(0, Ordering::AcqRel)),
    }
}

pub(super) fn publish_underrun_frames(ring: &SharedRing, frames: u64) {
    ring.underrun_frames.store(frames, Ordering::Release);
}

pub(super) fn underrun_frames(ring: &SharedRing) -> u64 {
    ring.underrun_frames.load(Ordering::Acquire)
}

/// instance ごとの auto-trim ゲインを dB で公開する。
///
/// 渡された数より後ろの instance は「auto gain が動いていない」= 0 dB に戻す。
/// track 数を減らしたあと、消えた行の古い値が残り続けるのを防ぐ。
pub(super) fn publish_auto_gain_db(ring: &SharedRing, gains_db: &[f32]) {
    for (slot, gain_db) in ring.auto_gain_db_bits.iter().zip(
        gains_db
            .iter()
            .copied()
            .chain(std::iter::repeat(0.0))
            .take(MAX_INSTANCE_COUNT),
    ) {
        slot.store(gain_db.to_bits(), Ordering::Release);
    }
}

/// instance ごとの auto-trim ゲイン（dB）。サーバーが公開している最新値。
pub(super) fn auto_gain_db(ring: &SharedRing) -> [f32; MAX_INSTANCE_COUNT] {
    let mut gains = [0.0; MAX_INSTANCE_COUNT];
    for (gain, slot) in gains.iter_mut().zip(ring.auto_gain_db_bits.iter()) {
        *gain = f32::from_bits(slot.load(Ordering::Acquire));
    }
    gains
}

fn update_atomic_max(value: &AtomicU32, candidate: f32) {
    let mut current = value.load(Ordering::Acquire);
    loop {
        if f32::from_bits(current) >= candidate {
            return;
        }
        match value.compare_exchange_weak(
            current,
            candidate.to_bits(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}
