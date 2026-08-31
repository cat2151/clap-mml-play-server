//! standby patch load の「完了通知」slot の読み書き。
//!
//! 汎用 request/response（[`super::protocol::ResponseSlot`]）は **受付応答** 専用に
//! なった。standby patch のロードは秒単位かかることがあり、その完了を汎用応答で
//! 待つとサーバーの受信ループが塞がって live timeline の MIDI が届かなくなる。
//! そこでロード結果だけを、サーバーが一方的に書きクライアントがポーリングで読む
//! 片方向 slot へ分離してある。
//!
//! 同期は [`SharedRing::standby_sequence`] の seqlock ただ 1 つ。
//!
//! - publish: `sequence` を奇数にする → body を書く → 偶数にする
//! - read: 偶数の `before` を読む → body を読む → `after` が `before` と同じなら採用
//!
//! body は [`std::cell::UnsafeCell`] で atomic ordering を持たないので、この順序が
//! 唯一の保証である。TUI 側 (`realtime-play/src/fast_midi_ipc/windows/standby.rs`)
//! と必ず同じ規約にすること。

use std::sync::atomic::Ordering;

use super::{
    protocol::{SharedRing, STANDBY_STATUS_ERROR, STANDBY_STATUS_SUCCESS},
    FastIpcError, MAX_STANDBY_ERROR_BYTES,
};

/// 書き換え中の body を読んでしまったときに諦めるまでの再試行回数。
///
/// publish は 1 KiB 程度の memcpy なので、この回数で足りなければ待つより
/// 呼び出し元へ「まだ」と返した方がよい。read 側は決して block しない。
const TORN_READ_RETRIES: usize = 64;

/// これから始める standby request の「基準 sequence」。
///
/// 完了通知は `sequence > watermark` のものだけを自分のものとして採用する。
/// request ID だけで判定すると、ID が wrap したときに過去の完了を自分の成功として
/// 拾ってしまう。sequence は単調増加なのでその取り違えが起きない。
///
/// 読んだ値が奇数（= publish 実行中）なら、その publish はこの request より前に
/// 始まっているので自分のものではありえない。偶数へ切り上げて「過去」に含める。
pub(super) fn standby_watermark(ring: &SharedRing) -> u64 {
    let sequence = ring.standby_sequence.load(Ordering::Acquire);
    (sequence + 1) & !1
}

/// standby patch load の結果を publish し、確定した偶数 sequence を返す。
///
/// メッセージが長すぎても publish を失敗させない。完了通知を落とすと、待っている
/// クライアントが永久に「ロード中」のまま残るため。UTF-8 境界で切り詰めるだけ。
pub(super) fn publish_standby_completion(
    ring: &SharedRing,
    request_id: u32,
    result: Result<(), &str>,
) -> u64 {
    let (status, message) = match result {
        Ok(()) => (STANDBY_STATUS_SUCCESS, ""),
        Err(message) => (STANDBY_STATUS_ERROR, message),
    };
    let payload = truncate_utf8(message, MAX_STANDBY_ERROR_BYTES).as_bytes();
    // odd: ここから body は不定。
    ring.standby_sequence.fetch_add(1, Ordering::AcqRel);
    unsafe {
        let slot = &mut *ring.standby.get();
        slot.request_id = request_id;
        slot.status = status;
        slot.payload_len = payload.len() as u32;
        slot.payload[..payload.len()].copy_from_slice(payload);
    }
    // even: body が確定した。
    ring.standby_sequence.fetch_add(1, Ordering::Release) + 1
}

/// `request_id` の完了通知を非 blocking に読む。
///
/// - `None`: まだ完了していない / 自分より前の古い完了 / 書き換え中
/// - `Some(Ok(()))`: ロード成功
/// - `Some(Err(_))`: ロード失敗、または slot が壊れている
///
/// 呼び出し元は `None` の間ポーリングを続ける。ここで待たないことがこの分離の
/// 目的そのものなので、block させないこと。
pub(super) fn read_standby_completion(
    ring: &SharedRing,
    request_id: u32,
    since_sequence: u64,
) -> Option<Result<(), FastIpcError>> {
    for _ in 0..TORN_READ_RETRIES {
        let before = ring.standby_sequence.load(Ordering::Acquire);
        if before & 1 != 0 {
            // publish 実行中。body は読まない。
            std::hint::spin_loop();
            continue;
        }
        if before == 0 {
            // まだ一度も publish されていない。
            return None;
        }
        // SAFETY: seqlock の read 側。`before` と `after` が一致したときだけ採用する。
        let snapshot = unsafe { StandbySnapshot::read(&*ring.standby.get()) };
        if ring.standby_sequence.load(Ordering::Acquire) != before {
            std::hint::spin_loop();
            continue;
        }
        if before <= since_sequence || snapshot.request_id != request_id {
            return None;
        }
        return Some(snapshot.into_result());
    }
    None
}

struct StandbySnapshot {
    request_id: u32,
    status: u32,
    payload_len: u32,
    payload: Vec<u8>,
}

impl StandbySnapshot {
    /// body をそのままコピーする。`payload_len` が壊れていても panic しないよう、
    /// 切り出す長さは必ず上限で clamp する。長さの検証は sequence 確認の後に行う。
    fn read(slot: &super::protocol::StandbyCompletionSlot) -> Self {
        let len = (slot.payload_len as usize).min(MAX_STANDBY_ERROR_BYTES);
        Self {
            request_id: slot.request_id,
            status: slot.status,
            payload_len: slot.payload_len,
            payload: slot.payload[..len].to_vec(),
        }
    }

    fn into_result(self) -> Result<(), FastIpcError> {
        if self.payload_len as usize > MAX_STANDBY_ERROR_BYTES {
            return Err(FastIpcError::InvalidPayload(
                "standby completion payload length is invalid".into(),
            ));
        }
        match self.status {
            STANDBY_STATUS_SUCCESS => Ok(()),
            STANDBY_STATUS_ERROR => Err(FastIpcError::RequestFailed(
                String::from_utf8_lossy(&self.payload).into_owned(),
            )),
            _ => Err(FastIpcError::InvalidPayload(
                "standby completion status is invalid".into(),
            )),
        }
    }
}

/// `max_bytes` を超えないよう UTF-8 の文字境界で切り詰める。
fn truncate_utf8(message: &str, max_bytes: usize) -> &str {
    if message.len() <= max_bytes {
        return message;
    }
    let mut end = max_bytes;
    while end > 0 && !message.is_char_boundary(end) {
        end -= 1;
    }
    &message[..end]
}

#[cfg(test)]
mod tests;
