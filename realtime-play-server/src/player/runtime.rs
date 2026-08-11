use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use cmrt_core::RealtimePlaybackSchedule;
use cmrt_realtime_ipc::{
    LimiterMeter, LiveTimelineConfig, TimelineMidiEvent, TimingMetrics, INSTANCE_COUNT,
};
use cmrt_timeline::{BlockScheduler, ConstantTempoTimeline, SampleRate, Timed, TimelineSeconds};

use super::auto_gain::InstanceAutoGain;

pub(super) enum PlaybackMode {
    Scheduled {
        generation: u64,
        playback: RealtimePlaybackSchedule,
    },
    Live {
        generation: u64,
        clock_samples: u64,
        instances: Vec<LiveInstanceState>,
        timeline: Option<LiveTimelineState>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TimelinePayload {
    pub(super) instance_id: u8,
    pub(super) message: [u8; 3],
}

pub(super) struct LiveTimelineState {
    pub(super) id: u64,
    pub(super) scheduler: BlockScheduler<TimelinePayload>,
    pub(super) transport: ConstantTempoTimeline,
    pub(super) started: bool,
}

impl LiveTimelineState {
    pub(super) fn new(config: LiveTimelineConfig) -> anyhow::Result<Self> {
        let sample_rate = SampleRate::new(config.sample_rate_hz)?;
        let transport = ConstantTempoTimeline::new(
            config.tempo_bpm,
            config.time_signature_numerator,
            config.time_signature_denominator,
        )
        .ok_or_else(|| anyhow::anyhow!("invalid live transport"))?;
        Ok(Self {
            id: config.timeline_id,
            scheduler: BlockScheduler::new(sample_rate),
            transport,
            started: false,
        })
    }

    pub(super) fn schedule(&mut self, event: TimelineMidiEvent) -> anyhow::Result<()> {
        if event.timeline_id != self.id {
            anyhow::bail!(
                "timeline mismatch: active={} event={}",
                self.id,
                event.timeline_id
            );
        }
        self.scheduler.schedule(Timed {
            at: TimelineSeconds::new(event.timeline_seconds)?,
            payload: TimelinePayload {
                instance_id: event.instance_id,
                message: event.message,
            },
        })?;
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct LiveInstanceState {
    pub(super) active: bool,
    pub(super) queue: Vec<LiveQueuedEvent>,
    pub(super) auto_gain: InstanceAutoGain,
}

/// auto-trim の on/off と、その結果として instance ごとに掛かっているゲイン。
///
/// ゲインを決めるのはワーカースレッドの [`InstanceAutoGain`] だが、共有メモリへ
/// 載せるのは IPC スレッドなので、間に atomic の写しを1枚挟む。
/// on/off と同じ構造体に置いているのは、「切ったのに値が残っている」状態を
/// 作らないため（[`Self::set_enabled`] が off で写しも 0 dB へ戻す）。
pub(super) struct AutoGainControl {
    enabled: AtomicBool,
    gains_db: [AtomicU32; INSTANCE_COUNT],
}

impl Default for AutoGainControl {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            gains_db: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
        }
    }
}

impl AutoGainControl {
    pub(super) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        if !enabled {
            self.clear_gains();
        }
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub(super) fn set_gain_db(&self, instance_id: usize, gain_db: f32) {
        if let Some(slot) = self.gains_db.get(instance_id) {
            slot.store(gain_db.to_bits(), Ordering::Release);
        }
    }

    pub(super) fn clear_gains(&self) {
        for slot in &self.gains_db {
            slot.store(0.0f32.to_bits(), Ordering::Release);
        }
    }

    pub(super) fn gains_db(&self) -> Vec<f32> {
        self.gains_db
            .iter()
            .map(|slot| f32::from_bits(slot.load(Ordering::Acquire)))
            .collect()
    }
}

pub(super) fn new_live_instances(count: usize) -> Vec<LiveInstanceState> {
    (0..count).map(|_| LiveInstanceState::default()).collect()
}

/// live mix で instance ごとに掛ける振幅ゲイン。
///
/// live モードは patch 差し替えのたびに作り直される（`new_live_mode`）ので、
/// ゲインは instance の状態ではなくここに置き、再生をまたいで保持する。
/// ワーカースレッドが毎チャンク読むため、ロックせず atomic に置く。
pub(super) struct LiveGains {
    gains: [AtomicU32; INSTANCE_COUNT],
}

impl Default for LiveGains {
    fn default() -> Self {
        Self {
            gains: std::array::from_fn(|_| AtomicU32::new(1.0f32.to_bits())),
        }
    }
}

impl LiveGains {
    pub(super) fn set(&self, instance_id: usize, gain: f32) {
        if let Some(slot) = self.gains.get(instance_id) {
            slot.store(gain.to_bits(), Ordering::Release);
        }
    }

    pub(super) fn get(&self, instance_id: usize) -> f32 {
        self.gains
            .get(instance_id)
            .map(|slot| f32::from_bits(slot.load(Ordering::Acquire)))
            .unwrap_or(1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LiveQueuedEvent {
    pub(crate) at_sample: u64,
    pub(crate) message: [u8; 3],
}

pub(super) struct LimiterMeterState {
    current_bits: AtomicU32,
    peak_bits: AtomicU32,
}

pub(super) struct TimingMetricsState {
    events: AtomicU64,
    late_events: AtomicU64,
    late_events_total: AtomicU64,
    max_late_samples: AtomicU64,
    max_late_us_bits: AtomicU64,
    output_lead_min_frames: AtomicU64,
    output_lead_max_frames: AtomicU64,
    process_load_p95_bits: AtomicU32,
    process_load_max_bits: AtomicU32,
}

impl Default for TimingMetricsState {
    fn default() -> Self {
        Self {
            events: AtomicU64::new(0),
            late_events: AtomicU64::new(0),
            late_events_total: AtomicU64::new(0),
            max_late_samples: AtomicU64::new(0),
            max_late_us_bits: AtomicU64::new(0.0f64.to_bits()),
            output_lead_min_frames: AtomicU64::new(0),
            output_lead_max_frames: AtomicU64::new(0),
            process_load_p95_bits: AtomicU32::new(0.0f32.to_bits()),
            process_load_max_bits: AtomicU32::new(0.0f32.to_bits()),
        }
    }
}

impl TimingMetricsState {
    pub(super) fn update(&self, metrics: TimingMetrics) {
        self.events.store(metrics.events, Ordering::Release);
        self.late_events
            .store(metrics.late_events, Ordering::Release);
        self.late_events_total
            .store(metrics.late_events_total, Ordering::Release);
        self.max_late_samples
            .store(metrics.max_late_samples, Ordering::Release);
        self.max_late_us_bits
            .store(metrics.max_late_us.to_bits(), Ordering::Release);
        self.output_lead_min_frames
            .store(metrics.output_lead_min_frames, Ordering::Release);
        self.output_lead_max_frames
            .store(metrics.output_lead_max_frames, Ordering::Release);
        self.process_load_p95_bits
            .store(metrics.process_load_p95.to_bits(), Ordering::Release);
        self.process_load_max_bits
            .store(metrics.process_load_max.to_bits(), Ordering::Release);
    }

    pub(super) fn snapshot(&self) -> TimingMetrics {
        TimingMetrics {
            events: self.events.load(Ordering::Acquire),
            late_events: self.late_events.load(Ordering::Acquire),
            late_events_total: self.late_events_total.load(Ordering::Acquire),
            max_late_samples: self.max_late_samples.load(Ordering::Acquire),
            max_late_us: f64::from_bits(self.max_late_us_bits.load(Ordering::Acquire)),
            output_lead_min_frames: self.output_lead_min_frames.load(Ordering::Acquire),
            output_lead_max_frames: self.output_lead_max_frames.load(Ordering::Acquire),
            process_load_p95: f32::from_bits(self.process_load_p95_bits.load(Ordering::Acquire)),
            process_load_max: f32::from_bits(self.process_load_max_bits.load(Ordering::Acquire)),
        }
    }
}

impl Default for LimiterMeterState {
    fn default() -> Self {
        Self {
            current_bits: AtomicU32::new(0),
            peak_bits: AtomicU32::new(0),
        }
    }
}

impl LimiterMeterState {
    pub(super) fn update(&self, current_db: f32, peak_db: f32) {
        self.current_bits
            .store(current_db.to_bits(), Ordering::Release);
        update_atomic_max(&self.peak_bits, peak_db);
    }

    pub(super) fn reset(&self) {
        self.current_bits.store(0, Ordering::Release);
        self.peak_bits.store(0, Ordering::Release);
    }

    pub(super) fn snapshot(&self) -> LimiterMeter {
        LimiterMeter {
            current_reduction_db: f32::from_bits(self.current_bits.load(Ordering::Acquire)),
            peak_reduction_db: f32::from_bits(self.peak_bits.swap(0, Ordering::AcqRel)),
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
