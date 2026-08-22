//! 差し替えと前払いにいくらかかるか。**実プラグインが要る**（回し方は親モジュールの doc）。
//!
//! ここは合否よりも出力する数字が本体で、ADR 0012 のベースラインはここから取る。

use super::*;

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

/// 3 種別構成の**起動直後の前払い**を、時間とメモリの両方で測る。
///
/// 予備の目標数はプラグインごとにスロット数ぶん（`spare_target`）なので、種別が増えると
/// 前払いの総量も増える。ここが実用外なら `CMRT_SPARE_INSTANCES` の既定を見直す判断材料になる。
/// **Vaporizer2 は 1 インスタンス約 89MB**（Surge XT / Dexed の約 12MB に対して）なので、
/// 効いてくるのは時間よりメモリのほう。
#[test]
#[ignore = "実プラグインが要る / 数十秒かかる"]
fn prepaying_spares_for_three_plugins_costs_this_much_time_and_memory() {
    const SLOTS: usize = 8;
    const PREPAY_LIMIT: Duration = Duration::from_secs(120);
    let kinds = real_kinds_with_vaporizer2();
    let names = kinds
        .iter()
        .map(|kind| kind.name.clone())
        .collect::<Vec<_>>();
    let startup_started = Instant::now();
    let mut renderers = create_live_renderers(&kinds[0], SLOTS).unwrap();
    let startup_ms = startup_started.elapsed().as_millis();
    let slots_only_working_set = working_set_mb();
    let mut instances = LiveInstances::new(kinds, SLOTS);
    let target = instances.spare_target;

    // 演奏しながら袋が埋まるのを待つ。本番でも前払いはアイドル中に消化される。
    let prepay = Instant::now();
    let mut filled_ms = vec![None; names.len()];
    filled_ms[0] = Some(0); // 既定プラグインは自給自足なので発注しない。
    while filled_ms.iter().any(Option::is_none) && prepay.elapsed() < PREPAY_LIMIT {
        render_idle(&mut renderers[0], 1);
        instances.collect_ready();
        for (kind, filled) in filled_ms.iter_mut().enumerate() {
            if filled.is_none() && instances.spares[kind].len() >= target {
                *filled = Some(prepay.elapsed().as_millis());
            }
        }
    }

    let spares = instances.spares.iter().map(Vec::len).collect::<Vec<_>>();
    eprintln!(
        "three_plugin_prepay slots={SLOTS} spare_target={target} startup_ms={startup_ms} \
         kinds={names:?} filled_ms={filled_ms:?} spares={spares:?} physical={} \
         working_set_slots_only={slots_only_working_set} working_set_prepaid={}",
        instances.physical_count(),
        working_set_mb(),
    );
    for (kind, name) in names.iter().enumerate() {
        assert_eq!(
            spares[kind],
            if kind == 0 { 0 } else { target },
            "{name} の予備が目標数まで埋まらなかった"
        );
    }
    // 前払いが終わったあとも演奏は続けられる（背景生成でインスタンスが壊れていない）。
    for (slot, renderer) in renderers.iter_mut().enumerate() {
        assert!(render_note(renderer) > 0.0, "slot {slot} が無音になった");
    }
}
