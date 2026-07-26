use std::collections::VecDeque;

const CEILING_DB: f32 = -1.0;
const LOOKAHEAD_SECONDS: f64 = 0.005;
const RELEASE_SECONDS: f64 = 0.100;

/// Master stereo peak limiter applied after all plugin instances have been mixed.
///
/// The detector is stereo-linked and looks ahead by 5 ms. Samples are delayed by
/// the same amount, so the gain calculated from the future peak can be applied
/// before that peak reaches the output.
pub(super) struct MasterLimiter {
    ceiling: f32,
    release_coefficient: f32,
    lookahead_frames: usize,
    delay: VecDeque<(f32, f32)>,
    detector: VecDeque<(usize, f32)>,
    next_frame: usize,
    gain: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct LimiterReduction {
    pub(super) current_db: f32,
    pub(super) peak_db: f32,
}

impl MasterLimiter {
    pub(super) fn new(sample_rate: f64) -> Self {
        let lookahead_frames = (sample_rate * LOOKAHEAD_SECONDS).round().max(1.0) as usize;
        let release_coefficient = (-1.0 / (sample_rate * RELEASE_SECONDS)).exp() as f32;
        Self {
            ceiling: 10.0f32.powf(CEILING_DB / 20.0),
            release_coefficient,
            lookahead_frames,
            delay: VecDeque::with_capacity(lookahead_frames + 1),
            detector: VecDeque::with_capacity(lookahead_frames + 1),
            next_frame: 0,
            gain: 1.0,
        }
    }

    pub(super) fn process(&mut self, samples: &mut [f32]) -> LimiterReduction {
        let mut peak_reduction = 0.0f32;
        for frame in samples.chunks_mut(2) {
            let left = frame[0];
            let right = frame.get(1).copied().unwrap_or(0.0);
            let peak = left.abs().max(right.abs());
            self.push_detector(peak);
            self.delay.push_back((left, right));

            let target_gain = self
                .detector
                .front()
                .map(|(_, peak)| {
                    if *peak > self.ceiling {
                        self.ceiling / *peak
                    } else {
                        1.0
                    }
                })
                .unwrap_or(1.0);
            if target_gain < self.gain {
                self.gain = target_gain;
            } else {
                self.gain = 1.0 - (1.0 - self.gain) * self.release_coefficient;
                self.gain = self.gain.min(target_gain);
            }

            let output = if self.delay.len() > self.lookahead_frames {
                self.delay.pop_front().unwrap_or((0.0, 0.0))
            } else {
                (0.0, 0.0)
            };
            frame[0] = output.0 * self.gain;
            if frame.len() > 1 {
                frame[1] = output.1 * self.gain;
            }

            let reduction = gain_reduction_db(self.gain);
            peak_reduction = peak_reduction.max(reduction);
            self.next_frame = self.next_frame.wrapping_add(1);
            self.expire_detector();
        }
        LimiterReduction {
            current_db: gain_reduction_db(self.gain),
            peak_db: peak_reduction,
        }
    }

    pub(super) fn reset(&mut self) {
        self.delay.clear();
        self.detector.clear();
        self.next_frame = 0;
        self.gain = 1.0;
    }

    fn push_detector(&mut self, peak: f32) {
        while self
            .detector
            .back()
            .is_some_and(|(_, queued_peak)| *queued_peak <= peak)
        {
            self.detector.pop_back();
        }
        self.detector.push_back((self.next_frame, peak));
    }

    fn expire_detector(&mut self) {
        let oldest = self.next_frame.saturating_sub(self.lookahead_frames);
        while self
            .detector
            .front()
            .is_some_and(|(index, _)| *index < oldest)
        {
            self.detector.pop_front();
        }
    }
}

fn gain_reduction_db(gain: f32) -> f32 {
    if gain >= 1.0 {
        0.0
    } else {
        -20.0 * gain.max(f32::MIN_POSITIVE).log10()
    }
}

#[cfg(test)]
mod tests;
