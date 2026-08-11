use std::time::{Duration, Instant};

use cmrt_realtime_ipc::TimingMetrics;
use cmrt_timeline::{BlockEvents, PlacementObservation, SampleRate, TimingDiagnostics};

use super::runtime::TimingMetricsState;

const WINDOW: Duration = Duration::from_secs(5);
const LOAD_BUCKETS: usize = 501;

pub(super) struct LiveTimingWindow {
    sample_rate: SampleRate,
    started: Instant,
    diagnostics: TimingDiagnostics,
    lead_min: u64,
    lead_max: u64,
    load_histogram: [u64; LOAD_BUCKETS],
    load_max: f32,
}

impl LiveTimingWindow {
    pub(super) fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate: SampleRate::new(sample_rate).expect("validated server sample rate"),
            started: Instant::now(),
            diagnostics: TimingDiagnostics::default(),
            lead_min: u64::MAX,
            lead_max: 0,
            load_histogram: [0; LOAD_BUCKETS],
            load_max: 0.0,
        }
    }

    pub(super) fn observe_events<T>(&mut self, events: &BlockEvents<T>) {
        for event in &events.events {
            let quantized_seconds = self.sample_rate.sample_to_seconds(event.quantized_sample);
            self.diagnostics.observe(PlacementObservation {
                quantization_error_seconds: quantized_seconds.get() - event.requested.get(),
                late_by_samples: event.late_by_samples,
                placement_error_samples: event.late_by_samples.min(i64::MAX as u64) as i64,
            });
        }
    }

    pub(super) fn observe_block(&mut self, elapsed: Duration, block_duration: Duration, lead: u64) {
        self.lead_min = self.lead_min.min(lead);
        self.lead_max = self.lead_max.max(lead);
        let load = if block_duration.is_zero() {
            0.0
        } else {
            elapsed.as_secs_f64() / block_duration.as_secs_f64() * 100.0
        };
        self.load_max = self.load_max.max(load as f32);
        let bucket = load.round().clamp(0.0, (LOAD_BUCKETS - 1) as f64) as usize;
        self.load_histogram[bucket] = self.load_histogram[bucket].saturating_add(1);
    }

    pub(super) fn publish_if_due(&mut self, state: &TimingMetricsState, now: Instant) {
        if now.saturating_duration_since(self.started) < WINDOW {
            return;
        }
        let window = self.diagnostics.take_window();
        let total = self.diagnostics.total();
        let metrics = TimingMetrics {
            events: window.events,
            late_events: window.late_events,
            late_events_total: total.late_events,
            max_late_samples: window.max_late_by_samples,
            max_late_us: window.max_late_by_samples as f64 / self.sample_rate.get() * 1_000_000.0,
            output_lead_min_frames: if self.lead_min != u64::MAX {
                self.lead_min
            } else {
                0
            },
            output_lead_max_frames: self.lead_max,
            process_load_p95: percentile(&self.load_histogram, 0.95) as f32,
            process_load_max: self.load_max,
        };
        state.update(metrics);
        eprintln!(
            "cmrt-timing: window_ms={} events={} late={} late_total={} late_max_samples={} \
             late_max_us={:.1} lead_frames={}..{} cpu_p95={:.0}% cpu_max={:.1}%",
            now.saturating_duration_since(self.started).as_millis(),
            metrics.events,
            metrics.late_events,
            metrics.late_events_total,
            metrics.max_late_samples,
            metrics.max_late_us,
            metrics.output_lead_min_frames,
            metrics.output_lead_max_frames,
            metrics.process_load_p95,
            metrics.process_load_max,
        );
        self.started = now;
        self.lead_min = u64::MAX;
        self.lead_max = 0;
        self.load_histogram.fill(0);
        self.load_max = 0.0;
    }
}

fn percentile(histogram: &[u64], percentile: f64) -> usize {
    let total = histogram.iter().sum::<u64>();
    if total == 0 {
        return 0;
    }
    let target = (total as f64 * percentile).ceil() as u64;
    let mut accumulated = 0u64;
    histogram
        .iter()
        .position(|count| {
            accumulated = accumulated.saturating_add(*count);
            accumulated >= target
        })
        .unwrap_or(histogram.len() - 1)
}

#[cfg(test)]
mod tests;
