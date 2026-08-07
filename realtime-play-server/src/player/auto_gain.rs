//! live instanceごとの軽量なRMS auto-trim。
//!
//! lookaheadやdelay lineは持たず、pluginが返した現在blockのRMSから、そのblockへ
//! 掛けるgain rampを決める。音声レイテンシは増やさない。

/// 全instanceをmixしたときに狙うRMS。instance数に応じて1instanceぶんを下げる。
const MIX_TARGET_RMS_DB: f32 = -12.0;
const SILENCE_GATE_POWER: f32 = 1.0e-6; // -60dBFS RMS
const MIN_GAIN_DB: f32 = -12.0;
const MAX_GAIN_DB: f32 = 6.0;
const ATTACK_DB_PER_SECOND: f32 = 120.0;
const RELEASE_DB_PER_SECOND: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GainRamp {
    pub(super) start: f32,
    pub(super) end: f32,
}

impl GainRamp {
    const UNITY: Self = Self {
        start: 1.0,
        end: 1.0,
    };

    pub(super) fn scaled(self, trim: f32) -> Self {
        Self {
            start: self.start * trim,
            end: self.end * trim,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct InstanceAutoGain {
    gain_db: f32,
    measured: bool,
}

impl InstanceAutoGain {
    pub(super) fn process_block(
        &mut self,
        samples: &[f32],
        sample_rate: f64,
        target_rms_db: f32,
        enabled: bool,
    ) -> GainRamp {
        if !enabled {
            self.reset();
            return GainRamp::UNITY;
        }
        let start = amplitude_from_db(self.gain_db);
        let Some(power) = block_power(samples) else {
            return GainRamp { start, end: start };
        };
        if power < SILENCE_GATE_POWER {
            // tailやrestでquiet patchと誤認しない。次の発音にも学習済みtrimを使う。
            return GainRamp { start, end: start };
        }

        let level_db = 10.0 * power.log10();
        let desired_db = (target_rms_db - level_db).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
        let next_db = if self.measured {
            slew_gain_db(
                self.gain_db,
                desired_db,
                block_seconds(samples, sample_rate),
            )
        } else {
            self.measured = true;
            // 最初のblockはloud patchだけ即座に抑える。quiet側は不完全なattack blockを
            // +6dBと誤認しやすいため、次block以降ゆっくり持ち上げる。
            desired_db.min(0.0)
        };
        self.gain_db = next_db;
        GainRamp {
            start,
            end: amplitude_from_db(next_db),
        }
    }

    pub(super) fn reset(&mut self) {
        self.gain_db = 0.0;
        self.measured = false;
    }
}

/// server instanceはGrid Sequencerの2bankぶんなので、論理track数でheadroomを割る。
pub(super) fn target_rms_db(physical_instance_count: usize) -> f32 {
    let logical_tracks = physical_instance_count.div_ceil(2).max(1);
    MIX_TARGET_RMS_DB - 10.0 * (logical_tracks as f32).log10()
}

fn block_power(samples: &[f32]) -> Option<f32> {
    if samples.is_empty() {
        return None;
    }
    let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
    let power = sum / samples.len() as f32;
    power.is_finite().then_some(power)
}

fn block_seconds(samples: &[f32], sample_rate: f64) -> f32 {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return 0.0;
    }
    (samples.len().div_ceil(2) as f64 / sample_rate) as f32
}

fn slew_gain_db(current: f32, desired: f32, seconds: f32) -> f32 {
    let rate = if desired < current {
        ATTACK_DB_PER_SECOND
    } else {
        RELEASE_DB_PER_SECOND
    };
    let limit = rate * seconds.max(0.0);
    current + (desired - current).clamp(-limit, limit)
}

fn amplitude_from_db(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests;
