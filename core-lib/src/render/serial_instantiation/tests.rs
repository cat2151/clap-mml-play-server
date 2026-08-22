use super::*;

use crate::{DEXED_PLUGIN_ID, SURGE_XT_PLUGIN_ID};

/// 落ちるのが実測で分かっているのは Vaporizer2 だけ。ここを広げると起動が遅くなる。
#[test]
fn only_vaporizer2_is_built_one_at_a_time() {
    assert!(plugin_requires_serial_instantiation(VAPORIZER2_PLUGIN_ID));
    assert!(!plugin_requires_serial_instantiation(SURGE_XT_PLUGIN_ID));
    assert!(!plugin_requires_serial_instantiation(DEXED_PLUGIN_ID));
    assert!(!plugin_requires_serial_instantiation(""));
}

/// 並列に作ってよいプラグイン同士は待ち合わせない。ここが exclusive になると、
/// Surge XT だけの構成の起動が 8 並列から 1 本へ落ちる（実測 530ms → 数秒）。
#[test]
fn two_parallel_safe_plugins_hold_the_lock_at_the_same_time() {
    let first = InstantiationPermit::acquire(SURGE_XT_PLUGIN_ID);
    let second = InstantiationPermit::acquire(DEXED_PLUGIN_ID);

    assert!(matches!(first, InstantiationPermit::Shared(_)));
    assert!(matches!(second, InstantiationPermit::Shared(_)));
}

/// 直列化が要るプラグインは write 側を取る。
///
/// 「同時に取れないこと」自体は別スレッドを待たせないと見えないので、ここでは
/// **どちらの側を取ったか**だけを見る（保持したまま二度取ると自分で止まる）。
#[test]
fn a_serial_plugin_takes_the_exclusive_side() {
    let permit = InstantiationPermit::acquire(VAPORIZER2_PLUGIN_ID);

    assert!(matches!(permit, InstantiationPermit::Exclusive(_)));
}

/// 環境変数の綴り。`off` / `0` / `false` 以外は「有効」。
///
/// **未設定が有効側**でなければならない。逆に倒すと、何も設定していない実運用が
/// そのまま落ちる。実プロセスの env を書き換えると並列に走る他のテストを壊すので、
/// 値だけを渡す形の判定を直接見る。
#[test]
fn the_escape_hatch_is_opt_in_and_case_insensitive() {
    assert!(serialization_enabled_for(None), "未設定なら直列化は有効");
    assert!(serialization_enabled_for(Some("")));
    assert!(serialization_enabled_for(Some("1")));
    assert!(serialization_enabled_for(Some("on")));
    for value in ["off", "OFF", "0", "false", " False "] {
        assert!(
            !serialization_enabled_for(Some(value)),
            "{value} は無効化の綴りとして扱われるべき"
        );
    }
}
