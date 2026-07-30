//! 起動時間の内訳を stderr へ出す計測ログ。
//!
//! クライアント（clap-mml-render-tui）は子プロセスの stderr を全行ログファイルへ
//! 転送するため、ここで `eprintln!` するだけで内訳が `log.txt` に残る。
//! `cmrt-server-startup: instances=N/total` はクライアントがパースしている
//! ので書式を変えず、計測は別プレフィックスで出す。

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

/// 計測行のプレフィックス。クライアント側の grep はこれを使う。
const TIMING_PREFIX: &str = "cmrt-server-timing:";

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
