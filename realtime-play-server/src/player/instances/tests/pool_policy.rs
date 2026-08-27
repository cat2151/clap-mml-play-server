//! 予備の在庫方針（何個持つか・いつ発注するか）。実プラグインは要らない。

use super::*;

/// 種別が 1 つなら、どの patch もプラグインをまたがない。背景スレッドも予備も要らない。
#[test]
fn a_single_plugin_setup_builds_no_spares_at_all() {
    let instances = live_instances(vec![fake_kind("Surge XT", PatchForm::StateFile, None)], 4);

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
    let instances = live_instances(kinds, 8);

    assert_eq!(instances.spare_target, 8);
    // 既定プラグイン（添字 0）は自給自足なので発注しない（§1.3）。
    assert_eq!(instances.outstanding[0], 0);
    assert_eq!(instances.outstanding[1], 8);
}

#[test]
fn floe_is_retained_as_its_own_spare_pool_kind() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, None),
        fake_kind("Dexed", PatchForm::Cartridge, None),
        fake_kind("Vaporizer2", PatchForm::Vvp, None),
        fake_kind("Floe", PatchForm::FloePreset, None),
    ];
    let instances = live_instances(kinds, 4);

    assert_eq!(instances.kinds.len(), 4);
    assert_eq!(instances.kinds[3].patch_form, PatchForm::FloePreset);
    assert_eq!(instances.outstanding, vec![0, 4, 4, 4]);
}

/// **受け入れ条件 8。** bank へ分けても予備の目標数の合計は分離前と同じ。
/// 合計が増えると物理インスタンス数とメモリがそのぶん増える。
#[test]
fn splitting_the_spare_target_across_banks_keeps_the_total() {
    for total in 0..=9 {
        let split = split_spare_target(total, [7, 7]);
        assert_eq!(split[0] + split[1], total, "total={total} で合計が変わった");
        // 端数は bank 0 側（BankLayout のスロットの割り方と同じ向き）。
        assert!(split[0] >= split[1], "total={total}");
    }
}

/// スロットが片方へ寄る構成（`CMRT_LIVE_INSTANCE_COUNT=1`）では、目標も寄せる。
#[test]
fn a_bank_without_slots_gets_no_spare_target() {
    assert_eq!(split_spare_target(8, [1, 0]), [8, 0]);
}

/// 前払いは 2 bank へ半分ずつ発注され、合計は分離前と同じ。
/// 背景生成の窓口はどちらの bank も持つ（持たない bank はプラグインをまたげない）。
#[test]
fn a_prepaid_spares_are_ordered_by_each_bank() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, None),
        fake_kind("Dexed", PatchForm::Cartridge, None),
    ];
    // grid sequencer の実運用（7 行 × 2 bank）。
    let banks = plan_bank_instances(kinds, [7, 7]).map(LiveInstances::new);

    assert_eq!(banks[0].spare_target, 4);
    assert_eq!(banks[1].spare_target, 4);
    assert_eq!(
        banks[0].spare_target + banks[1].spare_target,
        spare_target(14),
        "2 bank の合計が分離前の目標数と違う"
    );
    // 既定プラグイン（添字 0）は自給自足なので、どちらの bank も発注しない。
    assert_eq!(banks[0].outstanding, vec![0, 4]);
    assert_eq!(banks[1].outstanding, vec![0, 4]);
    assert!(banks.iter().all(|bank| bank.builder.is_some()));
}

/// 前払いの目標が 0 になった bank（割り当てが相方へ寄った側）でも、いま要る 1 本は
/// その場で発注する。**発注しなければ 20 秒待って timeout する**ので、その bank だけ
/// プラグインをまたげなくなる。
#[test]
fn a_bank_without_a_prepaid_target_orders_on_demand() {
    let kinds = vec![
        fake_kind("Surge XT", PatchForm::StateFile, None),
        fake_kind("Dexed", PatchForm::Cartridge, None),
    ];
    let [_bank0, builder] = spawn_builder(kinds.clone());
    let mut instances = LiveInstances::new(LiveInstancesSpec {
        bank: 1,
        kinds,
        slot_count: 1,
        spare_target: 0,
        builder: Some(builder),
    });
    assert_eq!(instances.outstanding, vec![0, 0], "前払いはしない");

    let started = Instant::now();
    let Err(error) = instances.take_spare(1) else {
        panic!("実在しない .clap から予備が作れてしまった");
    };

    // 実在しない `.clap` なので生成そのものは失敗する。ここで見たいのは
    // **待たずに失敗が返る**こと（発注していなければ SPARE_WAIT_TIMEOUT まで返らない）。
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "発注されず timeout まで待った: {}ms",
        started.elapsed().as_millis()
    );
    assert!(!error.is_empty());
}
