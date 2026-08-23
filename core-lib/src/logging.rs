//! Host process が決める診断出力先。
//!
//! server binary から使う場合は未注入のままなので stderr へ出す。TUI など別プロセスへ
//! 埋め込む場合は起動時に sink を注入し、alternate screen を壊さない出力先へ送る。

use std::sync::OnceLock;

pub type LogSink = fn(&str);

static LOG_SINK: OnceLock<LogSink> = OnceLock::new();

/// 埋め込み先 process が所有する診断 sink を注入する。
///
/// 最初の 1 件だけを採用する。未注入なら standalone server 向けに stderr を使う。
pub fn set_log_sink(sink: LogSink) {
    let _ = LOG_SINK.set(sink);
}

pub(crate) fn emit_diagnostic(message: impl AsRef<str>) {
    if let Some(sink) = LOG_SINK.get() {
        sink(message.as_ref());
    } else {
        eprintln!("{}", message.as_ref());
    }
}
