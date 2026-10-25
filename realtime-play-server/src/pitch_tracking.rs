//! note を変えたときに音高がついてくるか（`tracks_keyboard`）を、録った音声から推定する。
//! 周期性は累積平均正規化差分（YIN）で、補助に FFT のピークを見る。音色名は使わない。
use rustfft::{num_complex::Complex, FftPlanner};

pub(crate) const NOTES: [u8; 4] = [48, 49, 55, 60];
pub(crate) const SAMPLE_RATE: u32 = 48_000;
pub(crate) const TRACKING_TOLERANCE_CENTS: f64 = 40.0;

#[derive(Clone, Debug)]
pub(crate) struct Observation {
    pub(crate) note: u8,
    pub(crate) frequency_hz: Option<f64>,
    pub(crate) periodicity: f64,
    pub(crate) spectral_peaks: Vec<Peak>,
}

#[derive(Clone, Debug)]
pub(crate) struct Peak {
    pub(crate) frequency_hz: f64,
    pub(crate) energy_fraction: f64,
}

pub(crate) fn tracks_keyboard(observations: &[Observation]) -> bool {
    if observations.len() != NOTES.len() || observations.iter().zip(NOTES).any(|(o, n)| o.note != n)
    {
        return false;
    }
    let periodic = || {
        let Some(reference) = observations[0]
            .frequency_hz
            .filter(|f| f.is_finite() && *f > 0.0)
        else {
            return false;
        };
        observations.iter().all(|observation| {
            observation.periodicity.is_finite()
                && observation.periodicity >= 0.85
                && observation.frequency_hz.is_some_and(|frequency| {
                    frequency.is_finite()
                        && frequency > 0.0
                        && (1200.0 * (frequency / reference).log2()
                            - 100.0 * f64::from(observation.note - NOTES[0]))
                        .abs()
                            <= TRACKING_TOLERANCE_CENTS
                })
        })
    };
    periodic()
        || observations[0].spectral_peaks.iter().any(|reference| {
            reference.frequency_hz.is_finite()
                && reference.frequency_hz > 0.0
                && (0.08..=1.0).contains(&reference.energy_fraction)
                && observations.iter().all(|observation| {
                    observation.spectral_peaks.iter().any(|peak| {
                        peak.frequency_hz.is_finite()
                            && peak.frequency_hz > 0.0
                            && (0.08..=1.0).contains(&peak.energy_fraction)
                            && (1200.0 * (peak.frequency_hz / reference.frequency_hz).log2()
                                - 100.0 * f64::from(observation.note - NOTES[0]))
                            .abs()
                                <= TRACKING_TOLERANCE_CENTS
                    })
                })
        })
}

/// Inspect several post-attack windows. Keep the earliest clearly periodic one;
/// no peak from a noisy or silent window is allowed to imply a pitch.
pub(crate) fn observe(note: u8, stereo: &[f32]) -> Observation {
    let mono: Vec<f64> = stereo
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| chunk.iter().map(|v| f64::from(*v)).sum::<f64>() / 8.0)
        .collect(); // stereo downsampled by 4 with a box low-pass
    let spectral_peaks = mono.get(600..4696).map(spectral_peaks).unwrap_or_default();
    for start in [600, 1800, 3000] {
        // 50, 150, 250 ms at 12 kHz
        if let Some(window) = mono.get(start..start + 2048) {
            if let Some((frequency, periodicity)) = periodic_pitch(window, 12_000.0) {
                return Observation {
                    note,
                    frequency_hz: Some(frequency),
                    periodicity,
                    spectral_peaks,
                };
            }
        }
    }
    Observation {
        note,
        frequency_hz: None,
        periodicity: 0.0,
        spectral_peaks,
    }
}

fn spectral_peaks(samples: &[f64]) -> Vec<Peak> {
    if samples.iter().any(|x| !x.is_finite()) {
        return Vec::new();
    }
    let mut buffer: Vec<_> = samples
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let window =
                0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (samples.len() - 1) as f64).cos();
            Complex::new(value * window, 0.0)
        })
        .collect();
    FftPlanner::<f64>::new()
        .plan_fft_forward(buffer.len())
        .process(&mut buffer);
    let powers: Vec<_> = buffer[..buffer.len() / 2]
        .iter()
        .map(|v| v.norm_sqr())
        .collect();
    let total = powers.iter().sum::<f64>();
    if total < 1e-8 {
        return Vec::new();
    }
    let mut peaks = Vec::new();
    for bin in 2..powers.len() - 2 {
        let frequency = bin as f64 * 12000.0 / buffer.len() as f64;
        if !(35.0..=2000.0).contains(&frequency)
            || powers[bin] <= powers[bin - 1]
            || powers[bin] <= powers[bin + 1]
        {
            continue;
        }
        let (a, b, c) = (
            powers[bin - 1].max(1e-30).ln(),
            powers[bin].max(1e-30).ln(),
            powers[bin + 1].max(1e-30).ln(),
        );
        let denominator = a - 2.0 * b + c;
        let offset = if denominator.abs() > 1e-12 {
            (0.5 * (a - c) / denominator).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let energy_fraction = powers[bin - 1..=bin + 1].iter().sum::<f64>() / total;
        if energy_fraction >= 0.08 {
            peaks.push(Peak {
                frequency_hz: (bin as f64 + offset) * 12000.0 / buffer.len() as f64,
                energy_fraction,
            });
        }
    }
    peaks.sort_by(|a, b| b.energy_fraction.total_cmp(&a.energy_fraction));
    peaks.truncate(8);
    peaks
}

fn periodic_pitch(samples: &[f64], sample_rate: f64) -> Option<(f64, f64)> {
    if samples.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let energy = samples.iter().map(|x| x * x).sum::<f64>() / samples.len() as f64;
    if energy < 1e-10 {
        return None;
    }
    let max_lag = (sample_rate / 35.0) as usize;
    let min_lag = (sample_rate / 2000.0) as usize;
    let length = samples.len() - max_lag;
    let mut normalized = vec![1.0; max_lag + 1];
    let mut sum = 0.0;
    for lag in 1..=max_lag {
        let difference = (0..length)
            .map(|i| (samples[i] - samples[i + lag]).powi(2))
            .sum::<f64>();
        sum += difference;
        normalized[lag] = if sum > 0.0 {
            difference * lag as f64 / sum
        } else {
            1.0
        };
    }
    let mut lag = min_lag;
    while lag < max_lag {
        if normalized[lag] < 0.15 {
            while lag < max_lag && normalized[lag + 1] < normalized[lag] {
                lag += 1;
            }
            if lag == max_lag {
                return None;
            }
            let (left, center, right) = (normalized[lag - 1], normalized[lag], normalized[lag + 1]);
            let denominator = left - 2.0 * center + right;
            let offset = if denominator.abs() > 1e-12 {
                0.5 * (left - right) / denominator
            } else {
                0.0
            };
            return Some((
                sample_rate / (lag as f64 + offset.clamp(-0.5, 0.5)),
                1.0 - center,
            ));
        }
        lag += 1;
    }
    None
}

#[cfg(test)]
mod tests;
