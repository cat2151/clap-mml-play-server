use std::{
    cell::UnsafeCell,
    mem::size_of,
    sync::atomic::{AtomicU32, AtomicU64},
    time::Duration,
};

use super::{MAX_INSTANCE_COUNT, MAX_MIDI_MESSAGES, MAX_PATCH_BYTES, MAX_RESPONSE_BYTES};

pub(super) const MAGIC: [u8; 8] = *b"CMRTMIDI";
/// v8 adds live tempo-map changes ([`KIND_SET_LIVE_TEMPO`]).
pub(super) const VERSION: u32 = 8;
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
/// instance別RMS auto-trimのon/off。`buffer_multiplier`をbool（0/1）として流用する。
/// 構造体を変えないのでVERSIONは据え置ける。
pub(super) const KIND_SET_AUTO_GAIN: u32 = 8;
pub(super) const KIND_BEGIN_LIVE_TIMELINE: u32 = 9;
pub(super) const KIND_TIMELINE_MIDI: u32 = 10;
/// 走っている live timeline の tempo map へテンポ変化点を積む。
///
/// `timeline_id` / `tempo_bits` / `time_signature_*` に加えて
/// `timeline_seconds_bits[0]` を「この絶対秒から」として使う。構造体は変わらないが、
/// **このコマンドを知らないサーバーは黙って無視する**（テンポだけ古いまま鳴り続ける）。
/// 音量と違ってテンポは正しさの問題なので、VERSION を上げて繋がらないようにしてある。
pub(super) const KIND_SET_LIVE_TEMPO: u32 = 11;
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
    pub(super) time_signature_numerator: u32,
    pub(super) time_signature_denominator: u32,
    pub(super) timeline_id: u64,
    pub(super) sample_rate_bits: u64,
    pub(super) tempo_bits: u64,
    pub(super) messages: [[u8; 3]; MAX_MIDI_MESSAGES],
    pub(super) offsets: [u32; MAX_MIDI_MESSAGES],
    pub(super) timeline_seconds_bits: [u64; MAX_MIDI_MESSAGES],
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
    /// Even while stable, odd while the timing aggregate is being replaced.
    pub(super) timing_sequence: AtomicU64,
    pub(super) timing_events: AtomicU64,
    pub(super) timing_late_events: AtomicU64,
    pub(super) timing_late_events_total: AtomicU64,
    pub(super) timing_max_late_samples: AtomicU64,
    pub(super) timing_max_late_us_bits: AtomicU64,
    pub(super) timing_output_lead_min_frames: AtomicU64,
    pub(super) timing_output_lead_max_frames: AtomicU64,
    pub(super) timing_process_load_p95_bits: AtomicU32,
    pub(super) timing_process_load_max_bits: AtomicU32,
    /// instance ごとの auto-trim ゲイン（dB を `f32::to_bits` で運ぶ）。
    ///
    /// auto gain はサーバー内で毎ブロック動くので、クライアントは「効いているか」を
    /// 自前では知りようがない。リミッターメーターと同じく、サーバーが一方的に
    /// 書き、クライアントはポーリングで読むだけ。
    pub(super) auto_gain_db_bits: [AtomicU32; MAX_INSTANCE_COUNT],
    pub(super) response: UnsafeCell<ResponseSlot>,
    pub(super) slots: [UnsafeCell<CommandSlot>; SLOT_COUNT],
}

unsafe impl Sync for SharedRing {}

const _: () = assert!(size_of::<CommandSlot>() == 6208);
const _: () = assert!(size_of::<ResponseSlot>() == 16_396);
const _: () = assert!(size_of::<SharedRing>() == 414_016);
