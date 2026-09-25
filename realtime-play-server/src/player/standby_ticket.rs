//! 非演奏 bank への先読みロードの受付票。

use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};

use anyhow::Result;

/// 先読みロードの結果。ワーカー間は `String` で運ぶ（`anyhow::Error` は Send 境界を
/// 跨がせたくないため、既存の patch load 系と同じ形に揃えてある）。
pub(crate) type StandbyLoadResult = std::result::Result<(), String>;

/// 先読みロードの受付票。
///
/// [`PlayerHandle::begin_standby_live_patch`] が返す。ロードは対象 bank の worker
/// 上で走り続けていて、この受付票を持っているスレッド（fast IPC 受信スレッド）は
/// **待たずに他のコマンドを捌く**。
///
/// 完了送信路は容量 1 なので、受け取り手が poll していなくても coordinator 側の
/// `send` が block しない。受付票を drop してもロードは止まらない。
pub(crate) struct StandbyLoadTicket {
    completion: Receiver<StandbyLoadResult>,
}

/// 受付票と、その完了を送る側の組を作る。
///
/// 容量 1 の同期チャネルであることがこの設計の要。0（rendezvous）にすると
/// coordinator の `send` が受け取り手を待って止まり、レンダーループごと固まる。
pub(crate) fn standby_completion_channel() -> (SyncSender<StandbyLoadResult>, StandbyLoadTicket) {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    (tx, StandbyLoadTicket { completion: rx })
}

impl StandbyLoadTicket {
    /// 完了していれば結果を返す。**まだなら `None`。決して block しない。**
    ///
    /// 送信側が結果を送らずに消えた場合（ワーカー停止）も `Some(Err(_))` を返す。
    /// ここで `None` を返し続けると、クライアントが永久に「ロード中」のまま残る。
    pub(crate) fn poll(&self) -> Option<Result<()>> {
        match self.completion.try_recv() {
            Ok(result) => Some(result.map_err(anyhow::Error::msg)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                "realtime play worker exited while preloading a standby patch"
            ))),
        }
    }
}
