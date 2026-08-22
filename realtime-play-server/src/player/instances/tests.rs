//! 予備インスタンスプールの検証。
//!
//! # 実プラグインが要るテストについて
//! この計画で唯一の未知は「演奏中に `RendererHandoff` の unsafe な `!Send` 移送を
//! 踏み続けて大丈夫か」で、これは型でも単体テストでも確かめられない。実物で殴るしかない。
//!
//! ```text
//! CMRT_TEST_SURGE_CLAP=C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap
//! CMRT_TEST_DEXED_CLAP=C:\Program Files\Common Files\CLAP\Dexed.clap
//! CMRT_TEST_DEXED_CARTRIDGES=C:\Users\<user>\AppData\Roaming\DigitalSuburban\Dexed\Cartridges
//! CMRT_TEST_VAPORIZER2_CLAP=C:\Program Files\Common Files\CLAP\VASTvaporizer2.clap
//! CMRT_TEST_VAPORIZER2_PRESETS=<.vvp の置き場>
//! cargo test -p clap-mml-realtime-play-server -- --include-ignored --test-threads=1
//! ```
//!
//! 環境変数が無いテストは、黙って通さず panic させる（未検証を成功と誤認しないため）。

use std::path::Path;
use std::time::{Duration, Instant};

use cmrt_core::CoreConfig;
use cmrt_server_config::PatchForm;

use super::super::startup::create_live_renderers;
use super::*;

const SURGE_CLAP_ENV: &str = "CMRT_TEST_SURGE_CLAP";
const DEXED_CLAP_ENV: &str = "CMRT_TEST_DEXED_CLAP";
const DEXED_CARTRIDGES_ENV: &str = "CMRT_TEST_DEXED_CARTRIDGES";
const VAPORIZER2_CLAP_ENV: &str = "CMRT_TEST_VAPORIZER2_CLAP";
const VAPORIZER2_PRESETS_ENV: &str = "CMRT_TEST_VAPORIZER2_PRESETS";
const SURGE_PLUGIN_ID: &str = "org.surge-synth-team.surge-xt";
const DEXED_PLUGIN_ID: &str = "com.digital-suburban.dexed";
const VAPORIZER2_PLUGIN_ID: &str = "com.vastdynamics.VAST2";
const SAMPLE_RATE: f64 = 48_000.0;
const BUFFER_SIZE: usize = 512;

fn env_path(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} を設定してからこのテストを実行すること"))
}

fn test_core_cfg() -> CoreConfig {
    CoreConfig {
        sample_rate: SAMPLE_RATE,
        buffer_size: BUFFER_SIZE,
        ..Default::default()
    }
}

fn fake_kind(name: &str, patch_form: PatchForm, patches_dir: Option<&str>) -> PluginKind {
    PluginKind {
        name: name.to_string(),
        plugin_path: format!("{name}.clap"),
        patch_form,
        core_cfg: CoreConfig {
            patches_dir: patches_dir.map(str::to_string),
            ..test_core_cfg()
        },
    }
}

// ---- 実プラグインが要るテストの道具立て ----

/// Surge XT を既定、Dexed を追加の種別にした 2 種別構成。
fn real_kinds() -> Vec<PluginKind> {
    vec![
        PluginKind {
            name: "Surge XT".to_string(),
            plugin_path: env_path(SURGE_CLAP_ENV),
            patch_form: PatchForm::StateFile,
            core_cfg: CoreConfig {
                plugin_id: Some(SURGE_PLUGIN_ID.to_string()),
                ..test_core_cfg()
            },
        },
        PluginKind {
            name: "Dexed".to_string(),
            plugin_path: env_path(DEXED_CLAP_ENV),
            patch_form: PatchForm::Cartridge,
            core_cfg: CoreConfig {
                plugin_id: Some(DEXED_PLUGIN_ID.to_string()),
                patches_dir: Some(env_path(DEXED_CARTRIDGES_ENV)),
                ..test_core_cfg()
            },
        },
    ]
}

/// Dexed を既定、Surge XT を追加の種別にした構成。
///
/// 予備の補充コストが高いのは Surge XT 側（1 個 200〜500ms）なので、
/// 「待たされる側」を測るにはこちらの並びが要る。
fn real_kinds_dexed_default() -> Vec<PluginKind> {
    let mut kinds = real_kinds();
    kinds.swap(0, 1);
    kinds
}

/// Surge XT を既定、Dexed と Vaporizer2 を追加にした 3 種別構成。
///
/// **Vaporizer2 は instance のスレッド並列生成で落ちる**（ADR 0009 / 0012）。ホスト側で
/// 直列化してあるので、この構成が通ることが「直列化が本番の経路まで効いている」ことになる。
fn real_kinds_with_vaporizer2() -> Vec<PluginKind> {
    let mut kinds = real_kinds();
    kinds.push(PluginKind {
        name: "Vaporizer2".to_string(),
        plugin_path: env_path(VAPORIZER2_CLAP_ENV),
        patch_form: PatchForm::Vvp,
        core_cfg: CoreConfig {
            plugin_id: Some(VAPORIZER2_PLUGIN_ID.to_string()),
            patches_dir: Some(env_path(VAPORIZER2_PRESETS_ENV)),
            ..test_core_cfg()
        },
    });
    kinds
}

/// 音色置き場の中の最初の `.vvp` の絶対パス。
///
/// ファイル名を直書きしないのは、個人の音色置き場の中身に依存しないため。
fn first_vvp_patch() -> String {
    let dir = env_path(VAPORIZER2_PRESETS_ENV);
    let mut presets = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{dir} を読めない: {error}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("vvp"))
        })
        .collect::<Vec<_>>();
    presets.sort();
    presets
        .first()
        .unwrap_or_else(|| panic!("{dir} に .vvp が 1 つも無い"))
        .to_string_lossy()
        .into_owned()
}

/// 自プロセスの working set（MB）。物理インスタンスを増やしたときの実コストを見る。
#[cfg(windows)]
fn working_set_mb() -> String {
    let output = std::process::Command::new("tasklist")
        .args([
            "/FI",
            &format!("PID eq {}", std::process::id()),
            "/FO",
            "CSV",
            "/NH",
        ])
        .output();
    match output {
        // CSV の最終列がメモリ使用量。値そのものが桁区切りのカンマを含むので、
        // 区切りは `,` ではなく `","` で見る。
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .trim()
            .rsplit("\",\"")
            .next()
            .unwrap_or("?")
            .trim_matches('"')
            .to_string(),
        Err(error) => format!("(取得できず: {error})"),
    }
}

/// cartridge ディレクトリの中の最初の `.syx` の program 00 を指す patch 文字列。
fn first_cartridge_patch() -> String {
    let dir = env_path(DEXED_CARTRIDGES_ENV);
    let mut cartridges = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{dir} を読めない: {error}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("syx"))
        })
        .collect::<Vec<_>>();
    cartridges.sort();
    let cartridge = cartridges
        .first()
        .unwrap_or_else(|| panic!("{dir} に .syx が 1 つも無い"));
    Path::new(cartridge)
        .join("00 test")
        .to_string_lossy()
        .into_owned()
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

/// Note On → 10 block → Note Off。
fn render_note(renderer: &mut cmrt_core::RealtimeRenderer) -> f32 {
    let mut loudest = peak(&renderer.render_live_chunk(&[[0x90, 60, 100]]).unwrap());
    for _ in 0..10 {
        loudest = loudest.max(peak(&renderer.render_live_chunk(&[]).unwrap()));
    }
    let _ = renderer.render_live_chunk(&[[0x80, 60, 0]]).unwrap();
    loudest
}

fn render_idle(renderer: &mut cmrt_core::RealtimeRenderer, blocks: usize) -> f32 {
    let mut loudest = 0.0_f32;
    for _ in 0..blocks {
        loudest = loudest.max(peak(&renderer.render_live_chunk(&[]).unwrap()));
    }
    loudest
}

/// スロットへ patch を用意して、実際にロードするところまで。本番の
/// `PrepareLivePatch` と同じ手順にしてある。
fn prepare(
    instances: &mut LiveInstances,
    renderers: &mut [cmrt_core::RealtimeRenderer],
    slot: usize,
    patch: Option<&str>,
) {
    instances
        .prepare_slot_for_patch(renderers, slot, patch)
        .unwrap_or_else(|error| panic!("slot {slot} の差し替えに失敗: {error}"));
    renderers[slot].reset();
    renderers[slot]
        .set_patch(patch)
        .unwrap_or_else(|error| panic!("slot {slot} の音色ロードに失敗: {error:#}"));
}

// ---- テスト本体は責務ごとに分けてある（親には道具立てだけを置く） ----

/// 差し替えと前払いのコスト（時間・メモリ）の実測（実プラグインが要る）。
mod cost;
/// 実プラグインの要らない、袋の在庫方針そのもののテスト。
mod pool_policy;
/// 演奏中にプラグインをまたいで差し替え続けても壊れないか（実プラグインが要る）。
mod swapping;
