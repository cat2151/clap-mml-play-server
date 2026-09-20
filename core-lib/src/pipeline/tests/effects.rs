//! offline render への effect chain の適用。
//!
//! 実 plugin が要るものは `#[ignore]` で、パスを環境変数で受ける:
//!
//! ```text
//! CMRT_TEST_SURGE_CLAP=C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap
//! CMRT_TEST_SURGEFX_CLAP=C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT Effects.clap
//! CMRT_TEST_SURGEFX_PRESETS=%ProgramData%\Surge XT\fx_presets
//! CMRT_TEST_TONE3000_CLAP=C:\Program Files\Common Files\CLAP\TONE3000.clap
//! CMRT_TEST_TONE3000_PRESETS=%ProgramData%\TONE3000\Presets\Factory
//! cargo test -p cmrt-core --release pipeline::tests::effects -- --ignored --nocapture --test-threads=1
//! ```

use super::*;
use crate::audio_effect::{AudioEffectCatalog, AudioEffectPluginInfo};
use crate::effect::rms_dbfs;
use crate::host::load_entry;
use crate::surge_fx_preset::SURGE_FX_PLUGIN_ID;
use crate::tone3000_preset::TONE3000_PLUGIN_ID;

const SURGE_CLAP_ENV: &str = "CMRT_TEST_SURGE_CLAP";
const SURGEFX_CLAP_ENV: &str = "CMRT_TEST_SURGEFX_CLAP";
const SURGEFX_PRESETS_ENV: &str = "CMRT_TEST_SURGEFX_PRESETS";
const TONE3000_CLAP_ENV: &str = "CMRT_TEST_TONE3000_CLAP";
const TONE3000_PRESETS_ENV: &str = "CMRT_TEST_TONE3000_PRESETS";

const MML_BODY: &str = "t120 o4 l4 c r r r";
const CHAIN_JSON: &str =
    r#"{"effects after instrument": [{"Surge XT Effects preset": "Reverb 1/Cathedral 2.srgfx"}]}"#;
const TWO_STAGE_JSON: &str = r#"{"effects after instrument": [{"TONE3000 preset": "Bogner Fullstack"}, {"Surge XT Effects preset": "Reverb 1/Cathedral 2.srgfx"}]}"#;

fn env_path(env: &str) -> String {
    std::env::var(env)
        .unwrap_or_else(|_| panic!("{env} にパスを設定してからこのテストを実行すること"))
}

fn test_config() -> CoreConfig {
    CoreConfig {
        output_midi: String::new(),
        output_wav: String::new(),
        sample_rate: 48_000.0,
        buffer_size: 512,
        patch_path: None,
        patches_dir: None,
        random_patch: false,
        ..Default::default()
    }
}

fn effect_catalog() -> AudioEffectCatalog {
    AudioEffectCatalog::scan(vec![
        AudioEffectPluginInfo::new(
            "TONE3000",
            env_path(TONE3000_CLAP_ENV),
            TONE3000_PLUGIN_ID,
            env_path(TONE3000_PRESETS_ENV),
        ),
        AudioEffectPluginInfo::new(
            "Surge XT Effects",
            env_path(SURGEFX_CLAP_ENV),
            SURGE_FX_PLUGIN_ID,
            env_path(SURGEFX_PRESETS_ENV),
        ),
    ])
}

fn load_effect_entry(plugin: &AudioEffectPluginInfo) -> Result<PluginEntry> {
    load_entry(&plugin.plugin_path)
}

fn mml_with(json: &str) -> String {
    format!("{json} {MML_BODY}")
}

/// 後半 1 秒（音符が鳴り終わった後）の RMS。リバーブの尻尾はここに出る。
fn tail_dbfs(samples: &[f32]) -> f32 {
    let frames = samples.len() / 2;
    rms_dbfs(&samples[(frames - 48_000) * 2..])
}

#[test]
fn unsupported_route_rejects_a_chain_before_rendering() {
    let _guard = super::EnvVarGuard::set(
        "CMRT_BASE_DIR",
        std::env::temp_dir().join("cmrt_test_effects_unsupported"),
    );
    let error = prepare_cache_render(
        &mml_with(CHAIN_JSON),
        &test_config(),
        RenderOptions::default(),
        RenderEffects::unsupported(),
    )
    .err()
    .expect("chain 付きの MML は unsupported な経路で弾かれる");
    assert!(
        format!("{error:#}").contains("effects after instrument"),
        "{error:#}"
    );
}

#[test]
fn unsupported_route_still_accepts_mml_without_a_chain() {
    let _guard = super::EnvVarGuard::set(
        "CMRT_BASE_DIR",
        std::env::temp_dir().join("cmrt_test_effects_unsupported"),
    );
    for mml in [
        MML_BODY,
        r#"{"Surge XT patch": "x.fxp"} t120 c"#,
        "{broken json c",
    ] {
        prepare_cache_render(
            mml,
            &test_config(),
            RenderOptions::default(),
            RenderEffects::unsupported(),
        )
        .unwrap_or_else(|error| panic!("{mml}: {error:#}"));
    }
}

#[test]
fn supported_route_rejects_an_unlisted_preset_before_rendering() {
    let _guard = super::EnvVarGuard::set(
        "CMRT_BASE_DIR",
        std::env::temp_dir().join("cmrt_test_effects_unlisted"),
    );
    let catalog = AudioEffectCatalog::default();
    let effects = RenderEffects::new(&catalog, &load_effect_entry);
    let error = prepare_cache_render(
        &mml_with(CHAIN_JSON),
        &test_config(),
        RenderOptions::default(),
        effects,
    )
    .err()
    .expect("catalog に無い preset は render 前に弾かれる");
    assert!(
        format!("{error:#}").contains("Surge XT Effects preset"),
        "{error:#}"
    );
}

#[test]
#[ignore = "実プラグインが要る"]
fn cache_render_with_a_reverb_chain_differs_from_dry_and_both_are_audible() {
    let _guard = super::EnvVarGuard::set(
        "CMRT_BASE_DIR",
        std::env::temp_dir().join("cmrt_test_effects_cache_render"),
    );
    let entry = load_entry(&env_path(SURGE_CLAP_ENV)).unwrap();
    let catalog = effect_catalog();
    assert_eq!(catalog.plugins().len(), 2, "{:?}", catalog.skipped());
    let effects = RenderEffects::new(&catalog, &load_effect_entry);
    let options = RenderOptions::new().with_preroll_ms(100);

    let dry = mml_render_for_cache_with_effects(MML_BODY, &test_config(), &entry, options, effects)
        .unwrap();
    let started = std::time::Instant::now();
    let wet = mml_render_for_cache_with_effects(
        &mml_with(CHAIN_JSON),
        &test_config(),
        &entry,
        options,
        effects,
    )
    .unwrap();
    let wet_elapsed = started.elapsed();

    assert_eq!(dry.len(), wet.len(), "chain は長さを変えない");
    let dry_rms = rms_dbfs(&dry);
    let wet_rms = rms_dbfs(&wet);
    let dry_tail = tail_dbfs(&dry);
    let wet_tail = tail_dbfs(&wet);
    println!(
        "dry: rms {dry_rms:.1} dBFS / tail {dry_tail:.1} dBFS, wet: rms {wet_rms:.1} dBFS / tail {wet_tail:.1} dBFS ({wet_elapsed:?})"
    );
    assert!(dry_rms > -60.0, "dry が無音: {dry_rms}");
    assert!(wet_rms > -60.0, "wet が無音: {wet_rms}");
    // Cathedral 2 は mix が浅く、全体 RMS では 0.1 dB も動かない。chain が効いた証拠は
    // 音符が終わった後の区間（dry は無音、wet はリバーブの尻尾）で見る。
    assert!(
        wet_tail > dry_tail + 6.0,
        "リバーブの尻尾が出ていない: dry {dry_tail} / wet {wet_tail}"
    );
    assert!(wet_tail > -60.0, "尻尾が可聴でない: {wet_tail}");
}

#[test]
#[ignore = "実プラグインが要る"]
fn cache_render_with_a_two_stage_chain_is_audible() {
    let _guard = super::EnvVarGuard::set(
        "CMRT_BASE_DIR",
        std::env::temp_dir().join("cmrt_test_effects_two_stage"),
    );
    let entry = load_entry(&env_path(SURGE_CLAP_ENV)).unwrap();
    let catalog = effect_catalog();
    let effects = RenderEffects::new(&catalog, &load_effect_entry);
    let options = RenderOptions::new().with_preroll_ms(100);

    let dry = mml_render_for_cache_with_effects(MML_BODY, &test_config(), &entry, options, effects)
        .unwrap();
    let started = std::time::Instant::now();
    let wet = mml_render_for_cache_with_effects(
        &mml_with(TWO_STAGE_JSON),
        &test_config(),
        &entry,
        options,
        effects,
    )
    .unwrap();
    println!(
        "two-stage: dry {:.1} dBFS / wet {:.1} dBFS ({:?})",
        rms_dbfs(&dry),
        rms_dbfs(&wet),
        started.elapsed()
    );
    assert_eq!(dry.len(), wet.len());
    assert!(rms_dbfs(&wet) > -60.0);
    // TONE3000（NAM の amp）は全体の RMS を動かす。
    assert!((rms_dbfs(&dry) - rms_dbfs(&wet)).abs() > 0.5);
    assert!(tail_dbfs(&wet) > tail_dbfs(&dry) + 6.0);
}
