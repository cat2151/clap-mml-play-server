use std::sync::atomic::{AtomicU32, Ordering};

use cmrt_core::RealtimePlaybackSchedule;
use cmrt_realtime_ipc::LimiterMeter;

pub(super) enum PlaybackMode {
    Scheduled {
        generation: u64,
        playback: RealtimePlaybackSchedule,
    },
    Live {
        generation: u64,
        clock_samples: u64,
        instances: Vec<LiveInstanceState>,
    },
}

#[derive(Default)]
pub(super) struct LiveInstanceState {
    pub(super) active: bool,
    pub(super) queue: Vec<LiveQueuedEvent>,
}

pub(super) fn new_live_instances(count: usize) -> Vec<LiveInstanceState> {
    (0..count).map(|_| LiveInstanceState::default()).collect()
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
