#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlacementObservation {
    pub quantization_error_seconds: f64,
    pub late_by_samples: u64,
    pub placement_error_samples: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimingSnapshot {
    pub events: u64,
    pub late_events: u64,
    pub max_late_by_samples: u64,
    pub max_quantization_error_seconds: f64,
    pub max_placement_error_samples: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TimingDiagnostics {
    window: TimingSnapshot,
    total: TimingSnapshot,
}

impl TimingDiagnostics {
    pub fn observe(&mut self, observation: PlacementObservation) {
        observe(&mut self.window, observation);
        observe(&mut self.total, observation);
    }

    pub const fn window(&self) -> TimingSnapshot {
        self.window
    }

    pub const fn total(&self) -> TimingSnapshot {
        self.total
    }

    pub fn take_window(&mut self) -> TimingSnapshot {
        std::mem::take(&mut self.window)
    }
}

fn observe(snapshot: &mut TimingSnapshot, observation: PlacementObservation) {
    snapshot.events = snapshot.events.saturating_add(1);
    if observation.late_by_samples > 0 {
        snapshot.late_events = snapshot.late_events.saturating_add(1);
    }
    snapshot.max_late_by_samples = snapshot
        .max_late_by_samples
        .max(observation.late_by_samples);
    snapshot.max_quantization_error_seconds = snapshot
        .max_quantization_error_seconds
        .max(observation.quantization_error_seconds.abs());
    snapshot.max_placement_error_samples = snapshot
        .max_placement_error_samples
        .max(observation.placement_error_samples.unsigned_abs());
}
