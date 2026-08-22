//! `CoreConfig.plugin_id` が descriptor 選択まで届くこと。
//!
//! ライブとオフラインの両方の instance 生成経路を、同じ観点で 1 対ずつ押さえる。
//! 共通のヘルパと環境変数は親モジュールにある。

use super::*;
use crate::pipeline::mml_render_stateless;

/// `CoreConfig.plugin_id` が instance 生成側の descriptor 選択まで届くこと。
///
/// ここが届かないと、descriptor を複数持つ CLAP で「2 件あり決められない」と落ちる
/// （起動ログだけは `plugin_id` で 1 件に決まるので、食い違いが見えにくい）。
#[test]
#[ignore = "実プラグインが要る"]
fn live_instance_creation_honors_the_configured_plugin_id() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = test_config_with_plugin_id(DEXED_PLUGIN_ID);

    let mut renderer = RealtimeRenderer::new(&cfg, &entry).unwrap();
    let samples = render_live_note(&mut renderer);

    assert!(peak(&samples) > 0.0, "ライブ経路が無音になっている");
}

#[test]
#[ignore = "実プラグインが要る"]
fn offline_instance_creation_honors_the_configured_plugin_id() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = test_config_with_plugin_id(DEXED_PLUGIN_ID);

    let samples = mml_render_stateless("t120 o4 l4 c", &cfg, &entry).unwrap();

    assert!(peak(&samples) > 0.0, "オフライン経路が無音になっている");
}

/// config の `plugin_id` が CLAP の中身と食い違うときは、黙って別の音色で鳴らさず落とす。
/// エラーには実際にあった descriptor ID を出す（config の書き間違いと CLAP の
/// 入れ替わりを区別できるようにするため）。
#[test]
#[ignore = "実プラグインが要る"]
fn live_instance_creation_rejects_a_plugin_id_the_clap_does_not_have() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = test_config_with_plugin_id("com.example.not-installed");

    let Err(error) = RealtimeRenderer::new(&cfg, &entry) else {
        panic!("plugin_id が食い違うのに instance が作れてしまった");
    };
    let error = error.to_string();

    assert!(error.contains("com.example.not-installed"), "{error}");
    assert!(error.contains(DEXED_PLUGIN_ID), "{error}");
}

#[test]
#[ignore = "実プラグインが要る"]
fn offline_instance_creation_rejects_a_plugin_id_the_clap_does_not_have() {
    let path = plugin_path(DEXED_CLAP_ENV);
    let entry = load_entry(&path).unwrap();
    let cfg = test_config_with_plugin_id("com.example.not-installed");

    let error = mml_render_stateless("t120 o4 l4 c", &cfg, &entry)
        .unwrap_err()
        .to_string();

    assert!(error.contains("com.example.not-installed"), "{error}");
    assert!(error.contains(DEXED_PLUGIN_ID), "{error}");
}
