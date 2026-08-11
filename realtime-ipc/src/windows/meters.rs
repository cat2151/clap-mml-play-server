//! 共有メモリのメーター領域の読み書き。
//!
//! コマンドのリングと違い、ここはサーバーが一方的に書きクライアントが読むだけの
//! 一方通行の領域で、応答も同期も伴わない。書き手と読み手が同じ順序規約
//! （`Release` で書き `Acquire` で読む）を使うことだけが要件なので、
//! 両方の実装を1か所へ並べて置く。

use std::sync::atomic::{AtomicU32, Ordering};

use super::{protocol::SharedRing, LimiterMeter, TimingMetrics, MAX_INSTANCE_COUNT};

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

pub(super) fn publish_timing_metrics(ring: &SharedRing, metrics: TimingMetrics) {
    ring.timing_sequence.fetch_add(1, Ordering::AcqRel);
    ring.timing_events.store(metrics.events, Ordering::Relaxed);
    ring.timing_late_events
        .store(metrics.late_events, Ordering::Relaxed);
    ring.timing_late_events_total
        .store(metrics.late_events_total, Ordering::Relaxed);
    ring.timing_max_late_samples
        .store(metrics.max_late_samples, Ordering::Relaxed);
    ring.timing_max_late_us_bits
        .store(metrics.max_late_us.to_bits(), Ordering::Relaxed);
    ring.timing_output_lead_min_frames
        .store(metrics.output_lead_min_frames, Ordering::Relaxed);
    ring.timing_output_lead_max_frames
        .store(metrics.output_lead_max_frames, Ordering::Relaxed);
    ring.timing_process_load_p95_bits
        .store(metrics.process_load_p95.to_bits(), Ordering::Relaxed);
    ring.timing_process_load_max_bits
        .store(metrics.process_load_max.to_bits(), Ordering::Relaxed);
    ring.timing_sequence.fetch_add(1, Ordering::Release);
}

pub(super) fn timing_metrics(ring: &SharedRing) -> TimingMetrics {
    loop {
        let before = ring.timing_sequence.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }
        let metrics = TimingMetrics {
            events: ring.timing_events.load(Ordering::Relaxed),
            late_events: ring.timing_late_events.load(Ordering::Relaxed),
            late_events_total: ring.timing_late_events_total.load(Ordering::Relaxed),
            max_late_samples: ring.timing_max_late_samples.load(Ordering::Relaxed),
            max_late_us: f64::from_bits(ring.timing_max_late_us_bits.load(Ordering::Relaxed)),
            output_lead_min_frames: ring.timing_output_lead_min_frames.load(Ordering::Relaxed),
            output_lead_max_frames: ring.timing_output_lead_max_frames.load(Ordering::Relaxed),
            process_load_p95: f32::from_bits(
                ring.timing_process_load_p95_bits.load(Ordering::Relaxed),
            ),
            process_load_max: f32::from_bits(
                ring.timing_process_load_max_bits.load(Ordering::Relaxed),
            ),
        };
        if ring.timing_sequence.load(Ordering::Acquire) == before {
            return metrics;
        }
    }
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
