//! instrument の後段に直列で挿す CLAP audio effect のホスト。
//!
//! [`crate::RealtimeRenderer`] は note port を要求し、input port を 1 本しか渡さないので
//! effect には使えない（Surge XT Effects は note port 0 本、input port 2 本）。ここでは
//! note port を要求せず、input port を広告された本数ぶん全部渡す（port 0 に信号、残りは無音）。
//!
//! RT で呼べるのは [`EffectRenderer::process`] だけで、1 block を in-place で処理し、
//! 確保もログもしない。生成・preset 適用・activate は別の関数で RT の外で済ませる。
//! [`EffectChain`] は複数段を順に通し、合計 latency ぶん出力をずらして長さを揃える。

mod chain;
mod preset_load;
mod renderer;

pub use chain::{EffectChain, EffectTransport};
pub use renderer::{EffectProcessError, EffectRenderer};

/// interleaved stereo 全体の RMS を dBFS で返す。無音は `-inf`。
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -f32::INFINITY;
    }
    let sum: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    let rms = (sum / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        return -f32::INFINITY;
    }
    (20.0 * rms.log10()) as f32
}

#[cfg(test)]
mod tests;
