//! server プロセスが「どの実体を、どの版で」起動したかを stderr へ 1 行で残す。
//!
//! クライアント（clap-mml-render-tui）は子プロセスの stderr を全行 `log.txt` へ
//! 転送するため、ここで `eprintln!` するだけでログに残る。tui 側の実装は要らない。
//!
//! この 1 行が無かったために、install 済みの古い exe を掴んでいた事故で
//! 「MML overlay が無音」以上の手がかりが残らなかった。config を読むより前、
//! つまり「起動に失敗しうる処理より前」に出すこと。
//!
//! 書式は既存の `cmrt-server-startup:` / `cmrt-server-timing:` に合わせた
//! `プレフィックス + key=value` 列。クライアント側の grep はプレフィックスを使う。

use std::path::Path;

/// 起動ログのプレフィックス。クライアント側の grep はこれを使う。
const BOOT_PREFIX: &str = "cmrt-server-boot:";

/// ログに載せる commit hash の長さ。`git show` に渡せて、かつ 1 行が読める長さ。
const COMMIT_HASH_LOG_LEN: usize = 12;

/// 起動した実体と版を stderr へ 1 行で出す。`main()` の先頭で呼ぶこと。
pub fn log_boot(commit_hash: &str) {
    let exe = std::env::current_exe().ok();
    eprintln!("{}", boot_line(commit_hash, exe.as_deref()));
}

/// 起動できずに終わる理由を stderr へ 1 行で出す。
///
/// anyhow のエラー鎖は `{:#}` でも複数行になりうるが、ログは 1 行 1 イベントなので畳む。
pub fn log_boot_fatal(stage: &str, detail: &str) {
    eprintln!("{}", fatal_line(stage, detail));
}

fn boot_line(commit_hash: &str, exe: Option<&Path>) -> String {
    let exe = match exe {
        Some(path) => path.display().to_string(),
        None => "(不明)".to_owned(),
    };
    format!(
        "{BOOT_PREFIX} commit={} exe=\"{exe}\"",
        short_commit_hash(commit_hash)
    )
}

fn fatal_line(stage: &str, detail: &str) -> String {
    format!(
        "{BOOT_PREFIX} fatal={stage} detail=\"{}\"",
        one_line(detail)
    )
}

fn short_commit_hash(commit_hash: &str) -> &str {
    let commit_hash = commit_hash.trim();
    match commit_hash.char_indices().nth(COMMIT_HASH_LOG_LEN) {
        Some((index, _)) => &commit_hash[..index],
        None => commit_hash,
    }
}

/// 複数行を 1 行へ畳む。`"` は key="value" の切れ目を壊すので `'` へ寄せる。
fn one_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
        .replace('"', "'")
}

#[cfg(test)]
mod tests;
