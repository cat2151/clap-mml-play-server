use crate::TimelineSeconds;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransportSnapshot {
    pub song_seconds: TimelineSeconds,
    pub song_beats: f64,
    pub tempo_bpm: f64,
    pub bar_start_beats: f64,
    pub bar_number: i32,
    pub time_signature_numerator: u16,
    pub time_signature_denominator: u16,
    pub playing: bool,
}

pub trait TransportTimeline {
    fn snapshot_at(&self, at: TimelineSeconds) -> Option<TransportSnapshot>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FreeRunningTimeline;

impl TransportTimeline for FreeRunningTimeline {
    fn snapshot_at(&self, _at: TimelineSeconds) -> Option<TransportSnapshot> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstantTempoTimeline {
    tempo_bpm: f64,
    numerator: u16,
    denominator: u16,
}

impl ConstantTempoTimeline {
    pub fn new(tempo_bpm: f64, numerator: u16, denominator: u16) -> Option<Self> {
        (tempo_bpm.is_finite()
            && tempo_bpm > 0.0
            && numerator > 0
            && denominator > 0
            && denominator.is_power_of_two())
        .then_some(Self {
            tempo_bpm,
            numerator,
            denominator,
        })
    }
}

impl TransportTimeline for ConstantTempoTimeline {
    fn snapshot_at(&self, at: TimelineSeconds) -> Option<TransportSnapshot> {
        let beats = at.get() * self.tempo_bpm / 60.0;
        let beats_per_bar = f64::from(self.numerator) * 4.0 / f64::from(self.denominator);
        let bar = (beats / beats_per_bar).floor();
        Some(TransportSnapshot {
            song_seconds: at,
            song_beats: beats,
            tempo_bpm: self.tempo_bpm,
            bar_start_beats: bar * beats_per_bar,
            bar_number: bar.min(f64::from(i32::MAX)) as i32,
            time_signature_numerator: self.numerator,
            time_signature_denominator: self.denominator,
            playing: true,
        })
    }
}
