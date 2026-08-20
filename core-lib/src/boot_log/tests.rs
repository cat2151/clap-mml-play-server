use std::path::Path;

use super::{boot_line, fatal_line, short_commit_hash};

#[test]
fn boot_line_shows_the_short_commit_hash_and_the_running_exe() {
    assert_eq!(
        boot_line(
            "2667184e5d5f1b7b2f94aaf1d6cd74923d5584a5",
            Some(Path::new("/bin/clap-mml-realtime-play-server"))
        ),
        "cmrt-server-boot: commit=2667184e5d5f exe=\"/bin/clap-mml-realtime-play-server\""
    );
}

/// commit hash を取れないビルド（`build_commit_hash.rs` の fallback）でも行は出す。
/// 「版が不明」自体が読み手にとっての情報になる。
#[test]
fn boot_line_keeps_a_commit_hash_shorter_than_the_log_length() {
    assert_eq!(
        boot_line("unknown", Some(Path::new("/bin/server"))),
        "cmrt-server-boot: commit=unknown exe=\"/bin/server\""
    );
}

#[test]
fn boot_line_marks_an_unknown_exe_instead_of_dropping_the_line() {
    assert_eq!(
        boot_line("abc", None),
        "cmrt-server-boot: commit=abc exe=\"(不明)\""
    );
}

#[test]
fn short_commit_hash_does_not_split_a_multi_byte_character() {
    assert_eq!(
        short_commit_hash("あいうえおかきくけこさしすせそ"),
        "あいうえおかきくけこさし"
    );
}

/// anyhow の `{:#}` は「原因: 詳細」で改行を含みうる。ログは 1 行 1 イベントなので畳む。
#[test]
fn fatal_line_folds_a_multi_line_detail_into_one_line() {
    assert_eq!(
        fatal_line("config", "config.toml のプラグイン設定が不正\n\n  active_plugin が解決できません\n"),
        "cmrt-server-boot: fatal=config detail=\"config.toml のプラグイン設定が不正 / active_plugin が解決できません\""
    );
}

/// detail 内の `"` が key=\"value\" の切れ目を壊さないこと。
#[test]
fn fatal_line_replaces_double_quotes_in_the_detail() {
    assert_eq!(
        fatal_line("config", "plugin_path = \"missing.clap\" が見つかりません"),
        "cmrt-server-boot: fatal=config detail=\"plugin_path = 'missing.clap' が見つかりません\""
    );
}
