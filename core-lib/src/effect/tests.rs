//! `EffectRenderer` / `EffectChain` のテスト。
//!
//! 実 plugin が要るものは `#[ignore]` で、パスを環境変数で受ける:
//!
//! ```text
//! CMRT_TEST_SURGEFX_CLAP=C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT Effects.clap
//! CMRT_TEST_SURGEFX_PRESETS=%ProgramData%\Surge XT\fx_presets
//! CMRT_TEST_TONE3000_CLAP=C:\Program Files\Common Files\CLAP\TONE3000.clap
//! CMRT_TEST_TONE3000_PRESETS=%ProgramData%\TONE3000\Presets\Factory
//! CMRT_TEST_DRAGONFLY_DIR=C:\Program Files\Common Files\CLAP\dragonfly-reverb
//! cargo test -p cmrt-core --release effect -- --ignored --test-threads=1
//! ```
//!
//! 環境変数が無いテストは、黙って通さず panic させる（未検証を成功と誤認しないため）。

use super::*;
use crate::host::load_entry;

const SAMPLE_RATE: f64 = 48_000.0;
const BUFFER_SIZE: usize = 512;

fn plugin_path(env: &str) -> String {
    std::env::var(env)
        .unwrap_or_else(|_| panic!("{env} に CLAP のパスを設定してからこのテストを実行すること"))
}

/// 区間 `[start, end)`（frame 単位）の RMS を dBFS で返す。
fn rms_dbfs_frames(samples: &[f32], start: usize, end: usize) -> f32 {
    rms_dbfs(&samples[start * 2..end * 2])
}

#[test]
fn empty_chain_leaves_samples_untouched() {
    let mut chain = EffectChain::new(Vec::new()).unwrap();
    let original: Vec<f32> = (0..1001).map(|index| index as f32 * 0.001).collect();
    let mut samples = original.clone();
    chain
        .process_all(&mut samples, EffectTransport::default())
        .unwrap();
    assert_eq!(samples, original);
    assert_eq!(chain.latency(), 0);
    assert!(chain.is_empty());
}

#[test]
fn rms_dbfs_of_silence_is_negative_infinity() {
    assert_eq!(rms_dbfs(&[0.0; 8]), -f32::INFINITY);
    assert_eq!(rms_dbfs(&[]), -f32::INFINITY);
    assert!((rms_dbfs(&[1.0, -1.0, 1.0, -1.0]) - 0.0).abs() < 1e-5);
}

mod dragonfly;
mod surge_fx;
mod tone3000;
