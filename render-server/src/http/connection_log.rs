//! 接続 1 本の処理の節目（受理・request 読み取り・render 前後・応答書き込み）を
//! ミリ秒つきで stderr へ残す。
//!
//! 呼び出し側（clap-mml-render-tui）は stderr を自分のログへ中継するだけで、
//! transport error を見たら server を起こし直す。server 側で「どの接続がどの段階まで
//! 進んだか」が残っていないと、その transport error が probe 接続の空読みなのか、
//! render 中の切断なのかを区別できない。

use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::Instant,
};

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);
static LISTEN_STARTED: OnceLock<Instant> = OnceLock::new();

/// listen 開始時刻を固定する。以後の `since_listen_ms` はここからの経過。
pub(super) fn mark_listen_started() {
    let _ = LISTEN_STARTED.set(Instant::now());
}

fn since_listen_ms() -> u128 {
    LISTEN_STARTED
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
}

pub(super) struct ConnectionLog {
    id: u64,
    worker: usize,
    accepted: Instant,
}

impl ConnectionLog {
    /// 接続を受理した時点で作り、`event=accepted` を残す。
    pub(super) fn accepted(worker: usize) -> Self {
        let log = Self {
            id: NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed),
            worker,
            accepted: Instant::now(),
        };
        log.event("accepted", format_args!(""));
        log
    }

    pub(super) fn event(&self, event: &str, fields: fmt::Arguments<'_>) {
        eprintln!(
            "cmrt-render-server: conn={} worker={} event={event} since_listen_ms={} since_accept_ms={}{}{fields}",
            self.id,
            self.worker,
            since_listen_ms(),
            self.accepted.elapsed().as_millis(),
            if fields.as_str() == Some("") { "" } else { " " },
        );
    }
}
