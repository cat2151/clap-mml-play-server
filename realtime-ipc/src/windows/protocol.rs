use std::{
    cell::UnsafeCell,
    mem::size_of,
    sync::atomic::{AtomicU32, AtomicU64},
    time::Duration,
};

use super::{MAX_MIDI_MESSAGES, MAX_PATCH_BYTES, MAX_RESPONSE_BYTES};

pub(super) const MAGIC: [u8; 8] = *b"CMRTMIDI";
pub(super) const VERSION: u32 = 5;
pub(super) const SLOT_COUNT: usize = 64;
pub(super) const KIND_MIDI: u32 = 1;
pub(super) const KIND_STOP: u32 = 2;
pub(super) const KIND_SET_BUFFER_MULTIPLIER: u32 = 3;
pub(super) const KIND_PREPARE_PATCH: u32 = 4;
pub(super) const KIND_PROBE_PATCH: u32 = 5;
pub(super) const KIND_STOP_ALL: u32 = 6;
/// live mix の instance ゲイン設定。`instance_id` と、`buffer_multiplier` を
/// 千分率のゲイン（1000 = 等倍）として流用する。構造体を変えないので VERSION は据え置ける。
pub(super) const KIND_SET_INSTANCE_GAIN: u32 = 7;
pub(super) const RESPONSE_OK: u32 = 1;
pub(super) const RESPONSE_ERROR: u32 = 2;
pub(super) const SERVER_STALE_MS: u64 = 1_000;
pub(super) const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

#[repr(C)]
pub(super) struct CommandSlot {
    pub(super) kind: u32,
    pub(super) request_id: u32,
    pub(super) message_count: u32,
    pub(super) patch_len: u32,
    pub(super) has_patch: u32,
    pub(super) instance_id: u32,
    pub(super) buffer_multiplier: u32,
    pub(super) messages: [[u8; 3]; MAX_MIDI_MESSAGES],
    pub(super) offsets: [u32; MAX_MIDI_MESSAGES],
    pub(super) instance_ids: [u8; MAX_MIDI_MESSAGES],
    pub(super) patch: [u8; MAX_PATCH_BYTES],
}

#[repr(C)]
pub(super) struct ResponseSlot {
    pub(super) request_id: u32,
    pub(super) status: u32,
    pub(super) payload_len: u32,
    pub(super) payload: [u8; MAX_RESPONSE_BYTES],
}

#[repr(C, align(64))]
pub(super) struct SharedRing {
    pub(super) magic: [u8; 8],
    pub(super) version: u32,
    pub(super) _reserved: u32,
    pub(super) server_pid: AtomicU32,
    pub(super) client_pid: AtomicU32,
    pub(super) write_index: AtomicU32,
    pub(super) read_index: AtomicU32,
    pub(super) heartbeat_ms: AtomicU64,
    pub(super) response_sequence: AtomicU32,
    pub(super) limiter_current_bits: AtomicU32,
    pub(super) limiter_peak_bits: AtomicU32,
    pub(super) underrun_frames: AtomicU64,
    pub(super) response: UnsafeCell<ResponseSlot>,
    pub(super) slots: [UnsafeCell<CommandSlot>; SLOT_COUNT],
}

unsafe impl Sync for SharedRing {}

const _: () = assert!(size_of::<CommandSlot>() == 5148);
const _: () = assert!(size_of::<ResponseSlot>() == 16_396);
const _: () = assert!(size_of::<SharedRing>() == 345_984);
