//! 起動時間の内訳を stderr へ出す計測ログ。
//!
//! クライアント（clap-mml-render-tui）は子プロセスの stderr を全行ログファイルへ
//! 転送するため、ここで `eprintln!` するだけで内訳が `log.txt` に残る。
//! `cmrt-server-startup:` はクライアントが中央 overlay の段階表示に使う。
//! 完了後の所要時間は `cmrt-server-timing:` へ分け、同じ `phase` 名で対応付ける。

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

/// 計測行のプレフィックス。クライアント側の grep はこれを使う。
const TIMING_PREFIX: &str = "cmrt-server-timing:";
const STARTUP_PREFIX: &str = "cmrt-server-startup:";

static BOOT: OnceLock<Instant> = OnceLock::new();

/// プロセス起動時刻。初回呼び出し時点で確定するため、`main()` の先頭で呼ぶこと。
pub(crate) fn boot() -> Instant {
    *BOOT.get_or_init(Instant::now)
}

/// `key=value` 形式のフィールド列を、起動からの経過時間付きで出力する。
pub(crate) fn log(fields: &str) {
    eprintln!(
        "{TIMING_PREFIX} {fields} since_boot_ms={}",
        boot().elapsed().as_millis()
    );
}

/// 単一フェーズの所要時間を出力する。
pub(crate) fn log_phase(phase: &str, elapsed: Duration) {
    log(&format!("phase={phase} ms={}", elapsed.as_millis()));
}

/// 長い起動フェーズへ入る直前に出す。完了時間は同じ名前で [`log_phase`] が出す。
pub(crate) fn begin_startup_phase(phase: &str) {
    eprintln!("{}", startup_phase_line(phase, boot().elapsed()));
}

fn startup_phase_line(phase: &str, since_boot: Duration) -> String {
    format!(
        "{STARTUP_PREFIX} phase={phase} event=begin since_boot_ms={}",
        since_boot.as_millis()
    )
}

#[cfg(test)]
mod tests;
