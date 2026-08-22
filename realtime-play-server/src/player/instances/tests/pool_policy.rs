//! 予備の在庫方針（何個持つか・いつ発注するか）。実プラグインは要らない。

use super::*;

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
