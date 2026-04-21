use anyhow::Result;
use clack_host::prelude::PluginEntry;

use crate::midi::{parse_smf_bytes, TimedMidiEvent};
use crate::render::render_to_memory;
use crate::CoreConfig;

const RENDER_CHANNELS: u64 = 2;

/// レンダリング時に追加で適用する前処理。
///
/// `Millis(100)` を指定すると、MIDI イベントを 100ms 後ろへずらしてレンダリングし、
/// 生成後に先頭 100ms を切り落とす。プラグイン初期化直後の発音欠け対策を、通常再生と
/// キャッシュレンダリングの両方へ同じ形で差し込める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderPreroll {
    #[default]
    Disabled,
    Millis(u64),
    Samples(u64),
}

impl RenderPreroll {
    pub fn disabled() -> Self {
        Self::Disabled
    }

    pub fn from_millis(millis: u64) -> Self {
        Self::Millis(millis)
    }

    pub fn from_samples(samples: u64) -> Self {
        Self::Samples(samples)
    }

    fn samples(self, sample_rate: f64) -> u64 {
        match self {
            Self::Disabled => 0,
            Self::Samples(samples) => samples,
            Self::Millis(millis) => {
                let samples = sample_rate * millis as f64 / 1000.0;
                if !samples.is_finite() || samples <= 0.0 {
                    0
                } else if samples >= u64::MAX as f64 {
                    u64::MAX
                } else {
                    samples.ceil() as u64
                }
            }
        }
    }
}

/// MML レンダリングの追加オプション。
///
/// 既定値は現在の挙動を保つため preroll 無効。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderOptions {
    preroll: RenderPreroll,
}

impl RenderOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_preroll(mut self, preroll: RenderPreroll) -> Self {
        self.preroll = preroll;
        self
    }

    pub fn with_preroll_ms(self, millis: u64) -> Self {
        self.with_preroll(RenderPreroll::from_millis(millis))
    }

    pub fn preroll(&self) -> RenderPreroll {
        self.preroll
    }

    pub(crate) fn preroll_samples(&self, sample_rate: f64) -> u64 {
        self.preroll.samples(sample_rate)
    }
}

pub(crate) struct PreparedRenderInputs {
    pub(crate) patched_cfg: CoreConfig,
    pub(crate) events: Vec<TimedMidiEvent>,
    pub(crate) total_samples: u64,
    pub(crate) preroll_samples: u64,
}

pub(crate) fn prepare_render_inputs(
    smf_bytes: &[u8],
    patched_cfg: CoreConfig,
    options: RenderOptions,
) -> Result<PreparedRenderInputs> {
    let (events, total_samples) = parse_smf_bytes(smf_bytes, patched_cfg.sample_rate)?;
    let preroll_samples = options.preroll_samples(patched_cfg.sample_rate);
    let (events, total_samples) = apply_render_preroll(events, total_samples, preroll_samples);
    Ok(PreparedRenderInputs {
        patched_cfg,
        events,
        total_samples,
        preroll_samples,
    })
}

pub(crate) fn render_prepared_inputs(
    prepared: PreparedRenderInputs,
    entry: &PluginEntry,
) -> Result<Vec<f32>> {
    let PreparedRenderInputs {
        patched_cfg,
        events,
        total_samples,
        preroll_samples,
    } = prepared;
    let samples = render_to_memory(&patched_cfg, entry, events, total_samples)?;
    Ok(trim_render_preroll(samples, preroll_samples))
}

pub(crate) fn apply_render_preroll(
    events: Vec<TimedMidiEvent>,
    total_samples: u64,
    preroll_samples: u64,
) -> (Vec<TimedMidiEvent>, u64) {
    if preroll_samples == 0 {
        return (events, total_samples);
    }
    let events = events
        .into_iter()
        .map(|event| TimedMidiEvent {
            sample_pos: event.sample_pos.saturating_add(preroll_samples),
            message: event.message,
        })
        .collect();
    (events, total_samples.saturating_add(preroll_samples))
}

pub(crate) fn trim_render_preroll(samples: Vec<f32>, preroll_samples: u64) -> Vec<f32> {
    let trim_len = preroll_samples.saturating_mul(RENDER_CHANNELS);
    let trim_len = usize::try_from(trim_len).unwrap_or(usize::MAX);
    samples.into_iter().skip(trim_len).collect()
}
