use std::{
    cell::UnsafeCell,
    mem::{offset_of, size_of},
    sync::atomic::{AtomicU32, AtomicU64},
    time::Duration,
};

use super::{
    MAX_EFFECT_CHAIN_BYTES, MAX_INSTANCE_COUNT, MAX_MIDI_MESSAGES, MAX_PATCH_BYTES,
    MAX_RESPONSE_BYTES, MAX_STANDBY_ERROR_BYTES,
};

pub(super) const MAGIC: [u8; 8] = *b"CMRTMIDI";
/// v12 adds [`KIND_FADE_OUT_INSTANCES`]. The layout is unchanged, but an old server would
/// reject the kind and leave the previous line ringing, so the version is bumped.
///
/// v11 adds [`CommandSlot::effect_chain`]; an old server would ignore it and play dry.
///
/// v10 splits [`KIND_PREPARE_STANDBY_PATCH`] into "accept" and "complete".
///
/// The generic [`ResponseSlot`] now carries only the **acceptance** of a standby
/// request; the patch load result is published later into the dedicated
/// [`StandbyCompletionSlot`]. Layout aside, the *meaning* of the generic response
/// changed, so an old client talking to a new server would treat "accepted" as
/// "loaded" and start playing a bank whose patch is still loading. That silent
/// failure mode is why the version is bumped instead of reusing v9.
///
/// v9 added standby-bank patch preload ([`KIND_PREPARE_STANDBY_PATCH`]).
///
/// v8 was live tempo-map changes ([`KIND_SET_LIVE_TEMPO`]).
pub(super) const VERSION: u32 = 12;
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
/// 非演奏 bank への先読みロード。command slot の使い方は [`KIND_PREPARE_PATCH`] と
/// 同じで、違うのは「対象 instance が演奏していない bank にある」というクライアントの
/// 宣言だけ。
///
/// **応答の意味だけは [`KIND_PREPARE_PATCH`] と違う。** v10 以降、request/response slot に
/// 同期で返るのは **受付 ACK** であって、ロード完了ではない。ロード結果は
/// [`StandbyCompletionSlot`] を非 blocking に poll して受け取る。この 2 段階にしたのは、
/// 受信スレッドをロード完了まで塞ぐと、その間 timeline MIDI が一切 dispatch されず
/// 演奏中の音が伸びてしまうため。
///
/// **この KIND を知らない古いサーバーは "unknown command kind" を返す**ので、
/// 先読みが黙って効かないままになる。先読みできるかは演奏の連続性に直結するため、
/// VERSION を上げて繋がらないようにしてある。
pub(super) const KIND_PREPARE_STANDBY_PATCH: u32 = 12;
/// 指定した instance 群の fadeout。`message_count` 個の `instance_ids` と、
/// `buffer_multiplier` を fadeout の長さ（ミリ秒）として流用する。応答は無い。
pub(super) const KIND_FADE_OUT_INSTANCES: u32 = 13;
/// 汎用応答の status。
///
/// [`KIND_PREPARE_STANDBY_PATCH`] に限り、この `OK` は **受付完了** であって
/// ロード完了ではない。ロード結果は [`StandbyCompletionSlot`] 側で通知される。
/// 他の KIND では従来どおり処理完了を意味する。
pub(super) const RESPONSE_OK: u32 = 1;
pub(super) const RESPONSE_ERROR: u32 = 2;
/// standby 完了通知の status。0 は「まだ一度も publish されていない」であって
/// 完了値ではない。wire 上に `pending` という完了値は存在しない。
pub(super) const STANDBY_STATUS_SUCCESS: u32 = 1;
pub(super) const STANDBY_STATUS_ERROR: u32 = 2;
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
    pub(super) effect_chain_len: u32,
    pub(super) timeline_id: u64,
    pub(super) sample_rate_bits: u64,
    pub(super) tempo_bits: u64,
    pub(super) messages: [[u8; 3]; MAX_MIDI_MESSAGES],
    pub(super) offsets: [u32; MAX_MIDI_MESSAGES],
    pub(super) timeline_seconds_bits: [u64; MAX_MIDI_MESSAGES],
    pub(super) instance_ids: [u8; MAX_MIDI_MESSAGES],
    pub(super) patch: [u8; MAX_PATCH_BYTES],
    /// patch 系要求に同梱する effect chain（`"effects after instrument"` の値の JSON 文字列）。
    pub(super) effect_chain: [u8; MAX_EFFECT_CHAIN_BYTES],
}

#[repr(C)]
pub(super) struct ResponseSlot {
    pub(super) request_id: u32,
    pub(super) status: u32,
    pub(super) payload_len: u32,
    pub(super) payload: [u8; MAX_RESPONSE_BYTES],
}

/// standby patch load の完了通知。
///
/// 汎用 [`ResponseSlot`] と違い、これはサーバーが一方的に書きクライアントが
/// ポーリングで読むだけの片方向 slot で、request/response の往復を伴わない。
/// 同時に in-flight にできる standby request は 1 件だけという契約なので slot も 1 件。
#[repr(C)]
pub(super) struct StandbyCompletionSlot {
    pub(super) request_id: u32,
    /// [`STANDBY_STATUS_SUCCESS`] / [`STANDBY_STATUS_ERROR`]。0 は未 publish。
    pub(super) status: u32,
    pub(super) payload_len: u32,
    /// エラーメッセージ。成功時は長さ 0。
    pub(super) payload: [u8; MAX_STANDBY_ERROR_BYTES],
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
    /// [`SharedRing::standby`] の seqlock。安定時は偶数、書き換え中だけ奇数。
    ///
    /// publish 側は odd -> body -> even の順に進め、read 側は before / body / after
    /// の before == after かつ偶数のときだけ body を採用する。body は
    /// [`UnsafeCell`] なので atomic ordering を持たない。この publish/read 契約
    /// だけが唯一の同期であり、両 repository で同じ順序を守ること。
    ///
    /// 0 は「まだ一度も publish していない」。単調増加なので、クライアントは
    /// request 開始時の値を控えておけば、request ID が wrap しても
    /// 「開始前に publish された古い完了」を取り違えずに捨てられる。
    pub(super) standby_sequence: AtomicU64,
    pub(super) standby: UnsafeCell<StandbyCompletionSlot>,
    pub(super) response: UnsafeCell<ResponseSlot>,
    pub(super) slots: [UnsafeCell<CommandSlot>; SLOT_COUNT],
}

unsafe impl Sync for SharedRing {}

const _: () = assert!(size_of::<CommandSlot>() == 10_304);
// chain の長さは u64 の手前の詰め物だった 4 byte に入れてある。両 repository で同じ位置にする。
const _: () = assert!(offset_of!(CommandSlot, effect_chain_len) == 36);
const _: () = assert!(offset_of!(CommandSlot, effect_chain) == 6208);
const _: () = assert!(size_of::<StandbyCompletionSlot>() == 1036);
const _: () = assert!(size_of::<ResponseSlot>() == 16_396);
const _: () = assert!(size_of::<SharedRing>() == 677_184);
// standby 用フィールドは auto gain と汎用応答の間へ挿し込んである。ここがずれても
// magic と version は一致してしまうので、接続時には気づけない。両 repository で
// 同じオフセットになることを const で固定する。
const _: () = assert!(offset_of!(SharedRing, standby_sequence) == 264);
const _: () = assert!(offset_of!(SharedRing, standby) == 272);
const _: () = assert!(offset_of!(SharedRing, response) == 1308);
const _: () = assert!(offset_of!(SharedRing, slots) == 17_704);
