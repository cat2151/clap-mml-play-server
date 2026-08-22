//! 実プラグインを読み込む統合テスト。
//!
//! CLAP 本体が要るので通常の `cargo test` では走らない（すべて `#[ignore]`）。
//! 走らせるにはプラグインのパスを環境変数で渡す:
//!
//! ```text
//! CMRT_TEST_DEXED_CLAP=C:\Program Files\Common Files\CLAP\Dexed.clap
//! CMRT_TEST_SURGE_CLAP=C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap
//! CMRT_TEST_VAPORIZER2_CLAP=C:\Program Files\Common Files\CLAP\VASTvaporizer2.clap
//! CMRT_TEST_VAPORIZER2_PRESETS=<.vvp を置いてあるディレクトリ>
//! CMRT_TEST_DEXED_CARTRIDGES=%APPDATA%\DigitalSuburban\Dexed\Cartridges
//! cargo test -p cmrt-core -- --ignored --test-threads=1
//! ```
//!
//! **`--test-threads=1` は必須**。Vaporizer2 は instance 生成が並列に耐えず、
//! テストが 2 本同時に走るとプロセスごと落ちる。
//!
//! 環境変数が無いテストは、黙って通さず panic させる（未検証を成功と誤認しないため）。
//!
//! このファイルにはプラグインをまたいで使うヘルパだけを置く。テスト本体は
//! プラグイン別（[`dexed`] / [`surge`] / [`vaporizer2`]）と、
//! プラグインに依らない観点（[`plugin_id`] / [`cartridge`]）の module にある。

use super::*;
use crate::host::load_entry;

const DEXED_CLAP_ENV: &str = "CMRT_TEST_DEXED_CLAP";
const SURGE_CLAP_ENV: &str = "CMRT_TEST_SURGE_CLAP";
const SAMPLE_RATE: f64 = 48_000.0;
const BUFFER_SIZE: usize = 512;

const DEXED_PLUGIN_ID: &str = "com.digital-suburban.dexed";

fn plugin_path(env: &str) -> String {
    std::env::var(env)
        .unwrap_or_else(|_| panic!("{env} に CLAP のパスを設定してからこのテストを実行すること"))
}

fn test_config() -> CoreConfig {
    CoreConfig {
        output_midi: String::new(),
        output_wav: String::new(),
        sample_rate: SAMPLE_RATE,
        buffer_size: BUFFER_SIZE,
        patch_path: None,
        patches_dir: None,
        random_patch: false,
        ..Default::default()
    }
}

fn test_config_with_plugin_id(plugin_id: &str) -> CoreConfig {
    CoreConfig {
        plugin_id: Some(plugin_id.to_string()),
        ..test_config()
    }
}

/// descriptor 選択から capability probe までを、instance を 1 つ作って通す。
fn probe_plugin(env: &str) -> (SelectedDescriptor, PluginCapabilities) {
    let path = plugin_path(env);
    let entry = load_entry(&path).unwrap();
    let descriptor = select_descriptor(&entry, None).unwrap();
    let mut plugin_instance = create_plugin_instance_without_patch(&entry, &descriptor).unwrap();
    let capabilities = probe_capabilities(&mut plugin_instance, &descriptor).unwrap();
    (descriptor, capabilities)
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

/// Note On → 9 block → Note Off。報告書の live probe と同じ手順。
fn render_live_note(renderer: &mut RealtimeRenderer) -> Vec<f32> {
    let mut samples = renderer.render_live_chunk(&[[0x90, 60, 100]]).unwrap();
    for _ in 0..9 {
        samples.extend(renderer.render_live_chunk(&[]).unwrap());
    }
    samples.extend(renderer.render_live_chunk(&[[0x80, 60, 0]]).unwrap());
    samples
}

mod cartridge;
mod dexed;
mod plugin_id;
mod surge;
mod vaporizer2;
