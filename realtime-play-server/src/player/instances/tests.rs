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
const SURGE_PLUGIN_ID: &str = "org.surge-synth-team.surge-xt";
const DEXED_PLUGIN_ID: &str = "com.digital-suburban.dexed";
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

/// 種別が 1 つなら、どの patch もプラグインをまたがない。背景スレッドも予備も要らない。
#[test]
fn a_single_plugin_setup_builds_no_spares_at_all() {
    let instances = LiveInstances::new(vec![fake_kind("Surge XT", PatchForm::StateFile, None)], 4);

    assert_eq!(instances.spare_target, 0);
    assert!(instances.builder.is_none());
}

/// 予備の目標数は「スロット数ぶんを上限まで」。前払いの量がここで決まる。
#[test]
fn a_spare_target_prepays_one_per_slot_up_to_the_cap() {
    assert_eq!(spare_target(4), 4);
    assert_eq!(
        spare_target(MAX_DEFAULT_SPARE_TARGET),
        MAX_DEFAULT_SPARE_TARGET
    );
    // スロットより多く持っても同時に飛べる上限を超えるので、頭打ちにする。
    assert_eq!(spare_target(32), MAX_DEFAULT_SPARE_TARGET);
}

/// 前払いは起動時に目標数ぶんまとめて発注する。1 件ずつ積むと、受け取りが worker の
/// ループからしか走らないため、コマンド待ちでブロックしているアイドル中に止まる。
#[test]
fn a_prepaid_spares_are_all_ordered_up_front() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, None),
        fake_kind("Dexed", PatchForm::Cartridge, None),
    ];
    let instances = LiveInstances::new(kinds, 8);

    assert_eq!(instances.spare_target, 8);
    // 既定プラグイン（添字 0）は自給自足なので発注しない（§1.3）。
    assert_eq!(instances.outstanding[0], 0);
    assert_eq!(instances.outstanding[1], 8);
}

// ---- ここから下は実プラグインが要る ----

/// **予備プール方式の本丸。** 演奏しながら背景でインスタンスを作り続け、プラグインを
/// またいで差し替え続けても落ちないか。`RendererHandoff` の unsafe な移送を
/// 演奏中ずっと踏むことになるので、ここが黒なら予備プール方式ごと成立しない。
#[test]
#[ignore = "実プラグインが要る"]
fn swapping_plugins_under_a_running_render_loop_survives_many_cycles() {
    const CYCLES: usize = 20;
    const SLOTS: usize = 4;
    let cartridge = first_cartridge_patch();
    let kinds = real_kinds();
    let mut renderers = create_live_renderers(&kinds[0], SLOTS).unwrap();
    let mut instances = LiveInstances::new(kinds, SLOTS);

    let started = Instant::now();
    for cycle in 0..CYCLES {
        for slot in 0..SLOTS {
            // 偶数周は Dexed の cartridge、奇数周は音色無指定（既定の Surge XT）。
            let patch = (cycle % 2 == 0).then_some(cartridge.as_str());
            prepare(&mut instances, &mut renderers, slot, patch);
            assert!(
                render_note(&mut renderers[slot]) > 0.0,
                "cycle {cycle} slot {slot} が差し替え後に無音になった"
            );
        }
        // 差し替えの合間も演奏は続いている。背景生成と並走させる。
        for renderer in renderers.iter_mut() {
            render_idle(renderer, 4);
        }
        instances.collect_ready();
    }
    eprintln!(
        "swap cycles={CYCLES} slots={SLOTS} total_ms={} physical={}",
        started.elapsed().as_millis(),
        instances.physical_count()
    );
    drop(renderers);
    drop(instances);
}

/// 差し替えた行の音が破綻しないか。袋へ返す前に音を止めていないと、
/// 返した物理インスタンスが鳴りっぱなしのまま次の行で使われる。
#[test]
#[ignore = "実プラグインが要る"]
fn a_note_left_sounding_does_not_survive_the_trip_through_the_pool() {
    let cartridge = first_cartridge_patch();
    let kinds = real_kinds();
    let mut renderers = create_live_renderers(&kinds[0], 1).unwrap();
    let mut instances = LiveInstances::new(kinds, 1);

    // Surge の音を鳴らしっぱなしにしたまま Dexed へ差し替える。
    prepare(&mut instances, &mut renderers, 0, None);
    let _ = renderers[0].render_live_chunk(&[[0x90, 60, 100]]).unwrap();
    assert!(
        render_idle(&mut renderers[0], 4) > 0.0,
        "差し替え前から無音になっている"
    );
    prepare(&mut instances, &mut renderers, 0, Some(&cartridge));
    // 同じ Surge インスタンスが袋から戻ってくる（袋は LIFO）。
    prepare(&mut instances, &mut renderers, 0, None);

    let leaked = render_idle(&mut renderers[0], 16);

    assert!(leaked < 1.0e-4, "前の音が残っている: peak={leaked}");
}

/// 最悪ケース（論理スロット 32 が両プラグインへ振れる）の物理インスタンス数と所要時間。
/// 実用外なら予備プール方式そのものを見直す判断材料になる。
#[test]
#[ignore = "実プラグインが要る / 数分かかる"]
fn worst_case_thirty_two_slots_across_both_plugins() {
    const SLOTS: usize = 32;
    let cartridge = first_cartridge_patch();
    let kinds = real_kinds();
    let startup_started = Instant::now();
    let mut renderers = create_live_renderers(&kinds[0], SLOTS).unwrap();
    let startup_ms = startup_started.elapsed().as_millis();
    let mut instances = LiveInstances::new(kinds, SLOTS);

    // 全スロットを Dexed へ。予備は 1 つしか無いので、ほとんどが背景生成待ちになる。
    let to_dexed = Instant::now();
    for slot in 0..SLOTS {
        prepare(&mut instances, &mut renderers, slot, Some(&cartridge));
    }
    let to_dexed_ms = to_dexed.elapsed().as_millis();

    // 全スロットを Surge へ戻す。返ってきた Dexed が袋に積まれた状態から始まる。
    let to_surge = Instant::now();
    for slot in 0..SLOTS {
        prepare(&mut instances, &mut renderers, slot, None);
    }
    let to_surge_ms = to_surge.elapsed().as_millis();

    for (slot, renderer) in renderers.iter_mut().enumerate() {
        assert!(render_note(renderer) > 0.0, "slot {slot} が無音になった");
    }
    eprintln!(
        "worst_case slots={SLOTS} startup_ms={startup_ms} to_dexed_ms={to_dexed_ms} \
         to_surge_ms={to_surge_ms} physical={} working_set={}",
        instances.physical_count(),
        working_set_mb(),
    );
}

/// 予備が尽きた状態でも、背景生成を待って差し替えが完了する。
/// 待ち時間そのものが実用性の判断材料になるので出力する。
#[test]
#[ignore = "実プラグインが要る"]
fn a_swap_waits_for_the_background_build_when_the_pool_runs_dry() {
    const SLOTS: usize = 8;
    let cartridge = first_cartridge_patch();
    let kinds = real_kinds();
    let mut renderers = create_live_renderers(&kinds[0], SLOTS).unwrap();
    let mut instances = LiveInstances::new(kinds, SLOTS);

    let mut waits = Vec::new();
    for slot in 0..SLOTS {
        let started = Instant::now();
        prepare(&mut instances, &mut renderers, slot, Some(&cartridge));
        waits.push(started.elapsed().as_millis());
    }

    for (slot, renderer) in renderers.iter_mut().enumerate() {
        assert!(render_note(renderer) > 0.0, "slot {slot} が無音になった");
    }
    eprintln!("dry_pool_swap_ms={waits:?}");
}

/// 1 周ごとの自動抽選（cycle random）は 1 周で最大 7 行が別プラグインへ飛びうる。
/// BPM 130 / 16 ステップの 1 小節は約 1.85 秒しかないので、**補充コストが高い側**
/// （Surge XT）へ 7 行が同時に飛ぶ場合が予備の深さを決める。
///
/// 予備の目標数は `CMRT_SPARE_INSTANCES` で変えられる。深さを変えて 2 回走らせ、
/// 「袋が温まっていれば即時」「温まっていなければ 1 個あたり約 490ms 待つ」ことを見る。
#[test]
#[ignore = "実プラグインが要る"]
fn seven_rows_moving_to_the_expensive_plugin_at_once() {
    const ROWS: usize = 7;
    const WARMUP_LIMIT: Duration = Duration::from_secs(10);
    let kinds = real_kinds_dexed_default();
    let mut renderers = create_live_renderers(&kinds[0], ROWS).unwrap();
    let mut instances = LiveInstances::new(kinds, ROWS);
    let surge = 1;

    // 演奏しながら袋が溜まるのを待つ。本番でも小節をまたぐ間ずっと演奏は続いている。
    let warmup = Instant::now();
    while instances.spares[surge].len() < ROWS && warmup.elapsed() < WARMUP_LIMIT {
        render_idle(&mut renderers[0], 1);
        instances.collect_ready();
    }
    let warmed = instances.spares[surge].len();
    let warmup_ms = warmup.elapsed().as_millis();

    // 測りたいのは差し替えの待ち時間なので、要求は「Surge の音色の**形**」だけでよい。
    // 実在する `.fxp` を要求しないのは、Surge の音色置き場を指す環境変数を
    // このテストのためだけに増やさないため。音色は差し替え後に初期音色へ戻す。
    let burst = |instances: &mut LiveInstances, renderers: &mut [_], patch: Option<&str>| {
        let started = Instant::now();
        let mut waits = Vec::new();
        for slot in 0..ROWS {
            let row_started = Instant::now();
            instances
                .prepare_slot_for_patch(renderers, slot, patch)
                .unwrap_or_else(|error| panic!("slot {slot} の差し替えに失敗: {error}"));
            waits.push(row_started.elapsed().as_millis());
        }
        (started.elapsed().as_millis(), waits)
    };

    let (first_ms, first_waits) = burst(&mut instances, &mut renderers, Some("Keys/Piano.fxp"));
    // 全行を Dexed へ戻してから、もう一度同じ幅で Surge へ飛ばす。
    // 2 回目は袋を一巡した物理インスタンスが戻っているはずで、そこが定常状態。
    let cartridge = first_cartridge_patch();
    burst(&mut instances, &mut renderers, Some(&cartridge));
    let (second_ms, second_waits) = burst(&mut instances, &mut renderers, Some("Keys/Piano.fxp"));

    for (slot, renderer) in renderers.iter_mut().enumerate() {
        renderer.set_patch(None).unwrap();
        assert!(render_note(renderer) > 0.0, "slot {slot} が無音になった");
    }
    eprintln!(
        "cycle_random_burst rows={ROWS} spare_target={} warmed={warmed} warmup_ms={warmup_ms}          first_ms={first_ms} first_per_row={first_waits:?}          second_ms={second_ms} second_per_row={second_waits:?}",
        instances.spare_target
    );
}
