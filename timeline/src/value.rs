use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimelineError {
    InvalidSeconds(f64),
    InvalidSampleRate(f64),
    EmptyBlock,
    PositionOverflow,
}

impl fmt::Display for TimelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSeconds(value) => {
                write!(
                    f,
                    "timeline seconds must be finite and non-negative (got {value})"
                )
            }
            Self::InvalidSampleRate(value) => {
                write!(f, "sample rate must be finite and positive (got {value})")
            }
            Self::EmptyBlock => write!(f, "audio block must contain at least one frame"),
            Self::PositionOverflow => write!(f, "timeline position exceeds the sample range"),
        }
    }
}

impl std::error::Error for TimelineError {}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct TimelineSeconds(f64);

impl TimelineSeconds {
    pub const ZERO: Self = Self(0.0);

    pub fn new(value: f64) -> Result<Self, TimelineError> {
        if !value.is_finite() || value < 0.0 {
            return Err(TimelineError::InvalidSeconds(value));
        }
        // A single representation keeps wire comparisons and stable ordering unsurprising.
        Ok(Self(if value == 0.0 { 0.0 } else { value }))
    }

    pub const fn get(self) -> f64 {
        self.0
    }

    pub fn from_step(step: u64, bpm: f64, steps_per_beat: u32) -> Result<Self, TimelineError> {
        if !bpm.is_finite() || bpm <= 0.0 || steps_per_beat == 0 {
            return Err(TimelineError::InvalidSeconds(f64::NAN));
        }
        Self::new(step as f64 * 60.0 / (bpm * f64::from(steps_per_beat)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct SampleRate(f64);

impl SampleRate {
    pub fn new(value: f64) -> Result<Self, TimelineError> {
        if !value.is_finite() || value <= 0.0 {
            return Err(TimelineError::InvalidSampleRate(value));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> f64 {
        self.0
    }

    pub fn seconds_to_sample(self, time: TimelineSeconds) -> Result<SamplePosition, TimelineError> {
        let samples = time.get() * self.0;
        if !samples.is_finite() || samples > u64::MAX as f64 {
            return Err(TimelineError::PositionOverflow);
        }
        Ok(SamplePosition(samples.round() as u64))
    }

    pub fn sample_to_seconds(self, position: SamplePosition) -> TimelineSeconds {
        // SampleRate is positive and SamplePosition fits in u64, so this is finite for practical
        // audio rates and is safe to construct directly.
        TimelineSeconds(position.0 as f64 / self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SamplePosition(pub u64);

impl SamplePosition {
    pub const ZERO: Self = Self(0);

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockSpan {
    start: SamplePosition,
    frames: u32,
}

impl BlockSpan {
    pub fn new(start: SamplePosition, frames: u32) -> Result<Self, TimelineError> {
        if frames == 0 {
            return Err(TimelineError::EmptyBlock);
        }
        start
            .0
            .checked_add(u64::from(frames))
            .ok_or(TimelineError::PositionOverflow)?;
        Ok(Self { start, frames })
    }

    pub const fn start(self) -> SamplePosition {
        self.start
    }

    pub const fn frames(self) -> u32 {
        self.frames
    }

    pub const fn end(self) -> SamplePosition {
        SamplePosition(self.start.0 + self.frames as u64)
    }
}
