//! 実 Zebralette3 CLAP へ、本番と同じ経路（`switch_patch` と `activate()` 前のロード）で `.h2p:z` を流す。
//!
//! plugin は組み込みプロファイル `Zebralette3` から、音色置き場（`<DataPath>\Presets\Zebralette3`）は
//! 本番と同じ経路で registry から取る（[`presets_dir`]）。
//! plugin は他製品の `.h2p` も拒否せず読んでしまうので、取り違えを host 側の `#AM=` 検証が
//! 両向きとも止めることもここで確かめる。

use std::path::PathBuf;

use super::tyrelln6::{
    envelope_diff, render_c4, tyrelln6_entry, tyrelln6_renderer, AUDIBLE_PEAK,
    OTHER_PATCH_MIN_ENVELOPE_DIFF,
};
use super::*;
use cmrt_server_config::ZEBRALETTE3_PLUGIN_ID;

const FACTORY_PATCH_COUNT: usize = 382;
const PATCH_A: &str = "1 Poly Keys/Applicant Replicant.h2p";
/// 取り違えの確認に使う TyrellN6 の音色。
const TYRELLN6_PATCH: &str = "01 Basses/Abgrund.h2p";
/// 本番の音色切替（play server の bank worker）と同じ空回しブロック数。
const PATCH_SETTLE_BLOCKS: usize = 4;

fn presets_dir_of(profile: &str) -> PathBuf {
    let cfg = cmrt_server_config::ServerConfig::load()
        .expect("config.toml が読めること（先に clap-mml-render-tui を一度起動する）");
    PathBuf::from(
        cfg.patch_dirs_of(profile)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("registry の {profile} DataPath が読めない")),
    )
}

fn presets_dir() -> PathBuf {
    presets_dir_of("Zebralette3")
}

/// Zebralette3 の wire（`<絶対パス>:z`）。
fn z3_wire(path: &std::path::Path) -> String {
    format!("{}:z", path.to_string_lossy())
}

fn zebralette3_entry() -> PluginEntry {
    let path = cmrt_server_config::builtin_plugin_profiles()
        .remove("Zebralette3")
        .expect("組み込みプロファイル 'Zebralette3' が無い")
        .plugin_path;
    load_entry(&path).unwrap()
}

fn zebralette3_renderer(entry: &PluginEntry) -> RealtimeRenderer {
    RealtimeRenderer::new(&test_config_with_plugin_id(ZEBRALETTE3_PLUGIN_ID), entry).unwrap()
}

#[test]
#[ignore = "実 Zebralette3 CLAP が要る"]
fn zebralette3_descriptor_and_capabilities() {
    let path = cmrt_server_config::default_zebralette3_plugin_path();
    let report = probe_plugin_capabilities(path, None).unwrap();
    eprintln!(
        "zebralette3 selected={} main_output_channels={} extensions={:?} rejected={:?}",
        report.selected.id, report.main_output_channels, report.extensions, report.rejected,
    );
    assert_eq!(report.selected.id, ZEBRALETTE3_PLUGIN_ID);
    assert_eq!(report.rejected, None);
    assert_eq!(report.main_output_channels, 2);
    assert!(report.extensions.contains(&"clap.state".to_string()));
}

/// 382 件すべてを `switch_patch` でロードして C4 を鳴らす。無音の音色は失敗にせず列挙する。
#[test]
#[ignore = "実 Zebralette3 CLAP と音色置き場が要る"]
fn every_factory_patch_loads_through_switch_patch() {
    let root = presets_dir();
    let patches = crate::collect_patches(&root.to_string_lossy()).unwrap();
    assert_eq!(
        patches.len(),
        FACTORY_PATCH_COUNT,
        "音色の件数が違う: {}",
        root.display()
    );
    let mut renderer = zebralette3_renderer(&zebralette3_entry());

    let mut failed = Vec::new();
    let mut silent = Vec::new();
    for patch in &patches {
        let display = crate::to_relative(&root.to_string_lossy(), patch);
        if let Err(error) = renderer.switch_patch(Some(&z3_wire(patch)), true, PATCH_SETTLE_BLOCKS)
        {
            failed.push(format!("{display}: {error:#}"));
            continue;
        }
        if peak(&render_c4(&mut renderer)) <= AUDIBLE_PEAK {
            silent.push(display);
        }
    }

    eprintln!(
        "zebralette3 loaded={} failed={} silent={} {silent:#?}",
        patches.len() - failed.len(),
        failed.len(),
        silent.len(),
    );
    assert!(failed.is_empty(), "ロードに失敗: {failed:#?}");
}

/// config の `patch_path` に `.h2p:z` を書いた起動（`activate()` 前のロード）でも 1 音目から
/// その音色で鳴り、生成直後の音とは違う。
#[test]
#[ignore = "実 Zebralette3 CLAP と音色置き場が要る"]
fn a_zebralette3_patch_in_the_config_is_loaded_before_activate() {
    let entry = zebralette3_entry();
    let init_sound = render_c4(&mut zebralette3_renderer(&entry));
    let cfg = CoreConfig {
        patch_path: Some(z3_wire(&presets_dir().join(PATCH_A))),
        ..test_config_with_plugin_id(ZEBRALETTE3_PLUGIN_ID)
    };
    let mut renderer = RealtimeRenderer::new(&cfg, &entry).unwrap();
    let first_note = render_c4(&mut renderer);

    let changed = envelope_diff(&first_note, &init_sound);
    eprintln!("zebralette3 before activate {PATCH_A}: diff(first note, init)={changed:.5}");
    assert!(peak(&first_note) > AUDIBLE_PEAK, "無音: {PATCH_A}");
    assert!(
        changed >= OTHER_PATCH_MIN_ENVELOPE_DIFF,
        "生成直後と音が変わらない（{changed}）"
    );
}

/// TyrellN6 の `.h2p` を Zebralette3 へ、Zebralette3 の `.h2p` を TyrellN6 へ流そうとすると、
/// plugin へ送る前に `#AM=` の検証で止まる。
#[test]
#[ignore = "実 Zebralette3・TyrellN6 CLAP と両方の音色置き場が要る"]
fn the_other_products_h2p_is_refused_in_both_directions() {
    let z3_patch = presets_dir().join(PATCH_A);
    let n6_patch = presets_dir_of("TyrellN6").join(TYRELLN6_PATCH);

    let mut z3 = zebralette3_renderer(&zebralette3_entry());
    let error = z3
        .switch_patch(Some(&z3_wire(&n6_patch)), true, PATCH_SETTLE_BLOCKS)
        .unwrap_err();
    eprintln!("TyrellN6 patch -> Zebralette3: {error:#}");
    assert!(
        format!("{error:#}").contains("TyrellN6 の音色"),
        "{error:#}"
    );

    let mut n6 = tyrelln6_renderer(&tyrelln6_entry());
    let error = n6
        .switch_patch(
            Some(&wire(&z3_patch.to_string_lossy())),
            true,
            PATCH_SETTLE_BLOCKS,
        )
        .unwrap_err();
    eprintln!("Zebralette3 patch -> TyrellN6: {error:#}");
    assert!(
        format!("{error:#}").contains("Zebralette3 の音色"),
        "{error:#}"
    );
}
