use super::*;
use crate::windows::protocol::SharedRing;

/// 共有メモリを張らずに seqlock の規約だけを検証する。
/// レイアウトは `protocol.rs` の size assertion が別途固定している。
fn new_ring() -> Box<SharedRing> {
    // SAFETY: `SharedRing` は repr(C) で、全 0 はサーバーが mapping を初期化した
    // 直後の状態そのもの（`FastMidiServer::create` が `write_bytes(0)` する）。
    unsafe { Box::new(std::mem::zeroed::<SharedRing>()) }
}

fn set_body(ring: &SharedRing, request_id: u32, status: u32, payload_len: u32) {
    unsafe {
        let slot = &mut *ring.standby.get();
        slot.request_id = request_id;
        slot.status = status;
        slot.payload_len = payload_len;
    }
}

#[test]
fn unpublished_slot_reports_no_completion() {
    let ring = new_ring();
    let watermark = standby_watermark(&ring);
    assert_eq!(watermark, 0);
    assert!(read_standby_completion(&ring, 1, watermark).is_none());
}

#[test]
fn success_is_visible_only_after_the_watermark() {
    let ring = new_ring();
    let watermark = standby_watermark(&ring);
    let sequence = publish_standby_completion(&ring, 7, Ok(()));
    assert_eq!(sequence, 2);
    assert!(sequence > watermark);
    assert_eq!(read_standby_completion(&ring, 7, watermark), Some(Ok(())));
    // 同じ完了を何度読んでも同じ結果になる（ポーリング前提なので消費されない）。
    assert_eq!(read_standby_completion(&ring, 7, watermark), Some(Ok(())));
}

#[test]
fn error_message_is_carried_by_the_completion_slot() {
    let ring = new_ring();
    let watermark = standby_watermark(&ring);
    publish_standby_completion(&ring, 3, Err("patch not found: Keys/Missing.fxp"));
    assert_eq!(
        read_standby_completion(&ring, 3, watermark),
        Some(Err(FastIpcError::RequestFailed(
            "patch not found: Keys/Missing.fxp".into()
        )))
    );
}

#[test]
fn completion_for_another_request_id_is_ignored() {
    let ring = new_ring();
    let watermark = standby_watermark(&ring);
    publish_standby_completion(&ring, 41, Ok(()));
    assert!(read_standby_completion(&ring, 42, watermark).is_none());
}

/// request ID は u32 で wrap する。ID だけで判定すると、前回 cycle の完了を
/// 今回の成功として拾ってしまう。watermark がそれを防ぐ。
#[test]
fn completion_published_before_the_watermark_is_not_reused_after_id_wrap() {
    let ring = new_ring();
    publish_standby_completion(&ring, 9, Ok(()));
    let watermark = standby_watermark(&ring);
    assert!(read_standby_completion(&ring, 9, watermark).is_none());

    // 同じ ID で新しく publish された分は採用される。
    publish_standby_completion(&ring, 9, Ok(()));
    assert_eq!(read_standby_completion(&ring, 9, watermark), Some(Ok(())));
}

/// publish 中（sequence が奇数）の body は決して採用しない。
#[test]
fn torn_write_is_never_observed_as_a_completion() {
    let ring = new_ring();
    let watermark = standby_watermark(&ring);
    // publish の途中を再現する: odd にして body だけ書いた状態。
    ring.standby_sequence.fetch_add(1, Ordering::AcqRel);
    set_body(&ring, 5, STANDBY_STATUS_SUCCESS, 0);
    assert!(read_standby_completion(&ring, 5, watermark).is_none());

    // publish が完了して偶数になった時点で初めて見える。
    ring.standby_sequence.fetch_add(1, Ordering::Release);
    assert_eq!(read_standby_completion(&ring, 5, watermark), Some(Ok(())));
}

/// 奇数のまま watermark を取ると、その publish は「自分より前」に含まれる。
#[test]
fn watermark_rounds_an_in_flight_publish_into_the_past() {
    let ring = new_ring();
    ring.standby_sequence.fetch_add(1, Ordering::AcqRel);
    set_body(&ring, 5, STANDBY_STATUS_SUCCESS, 0);
    let watermark = standby_watermark(&ring);
    assert_eq!(watermark, 2);
    ring.standby_sequence.fetch_add(1, Ordering::Release);
    assert!(read_standby_completion(&ring, 5, watermark).is_none());
}

#[test]
fn oversize_error_message_is_truncated_on_a_char_boundary() {
    let ring = new_ring();
    let watermark = standby_watermark(&ring);
    let message = "あ".repeat(MAX_STANDBY_ERROR_BYTES);
    publish_standby_completion(&ring, 1, Err(&message));
    let Some(Err(FastIpcError::RequestFailed(observed))) =
        read_standby_completion(&ring, 1, watermark)
    else {
        panic!("expected a truncated error completion");
    };
    assert!(observed.len() <= MAX_STANDBY_ERROR_BYTES);
    assert!(observed.len() > MAX_STANDBY_ERROR_BYTES - 3);
    assert!(message.starts_with(&observed));
    // 途中で切っても不正な UTF-8 にはしない（`from_utf8_lossy` の U+FFFD が出ない）。
    assert!(!observed.contains('\u{fffd}'));
}

#[test]
fn corrupt_payload_length_fails_the_request_instead_of_hanging() {
    let ring = new_ring();
    let watermark = standby_watermark(&ring);
    publish_standby_completion(&ring, 2, Ok(()));
    set_body(
        &ring,
        2,
        STANDBY_STATUS_ERROR,
        MAX_STANDBY_ERROR_BYTES as u32 + 1,
    );
    assert!(matches!(
        read_standby_completion(&ring, 2, watermark),
        Some(Err(FastIpcError::InvalidPayload(_)))
    ));
}

#[test]
fn unknown_status_fails_the_request_instead_of_hanging() {
    let ring = new_ring();
    let watermark = standby_watermark(&ring);
    publish_standby_completion(&ring, 2, Ok(()));
    set_body(&ring, 2, 99, 0);
    assert!(matches!(
        read_standby_completion(&ring, 2, watermark),
        Some(Err(FastIpcError::InvalidPayload(_)))
    ));
}
