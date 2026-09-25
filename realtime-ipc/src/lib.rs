//! Low-latency MIDI IPC used by clap-mml-render-tui and the realtime play server.

use std::fmt;

/// 共有メモリプロトコルが表現できる最大 instance 数。
///
/// server が実際に生成する数は起動時設定でこの上限以下にできる。
///
/// grid sequencer の chord mode は N トラックを 2 つの bank（= 2N instance）へ割り当て、
/// 鳴っている bank の裏でもう一方へ次の patch を先読みする。16 トラックぶんを
/// ダブルバッファにすると 32 必要なのでここが上限になる。
/// `instance_id` は wire format 上 `u32` / `u8` なので、この定数を上げても
/// 共有メモリのレイアウトは変わらない（`windows/protocol.rs` の `VERSION` は据え置きでよい）。
pub const MAX_INSTANCE_COUNT: usize = 32;
/// 旧 API との互換用。固定 wire format 上の instance 数を表す。
pub const INSTANCE_COUNT: usize = MAX_INSTANCE_COUNT;
pub const MAX_MIDI_MESSAGES: usize = 128;
pub const MAX_PATCH_BYTES: usize = 4096;
/// 音色の準備に同梱する effect chain（MML 先頭 JSON の `"effects after instrument"` の値を
/// JSON 文字列にしたもの）の最大バイト数。
pub const MAX_EFFECT_CHAIN_BYTES: usize = 4096;
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024;
/// standby 完了通知が運べるエラーメッセージの最大バイト数。
///
/// 汎用応答と違い、この slot は共有メモリに常設される固定長領域なので小さく取る。
/// これを超えるメッセージは publish 時に UTF-8 境界で切り詰められる。完了通知を
/// 落とすと先読みが永久に Loading のまま残るので、長すぎることを理由に
/// publish を失敗させない。TUI 側の `cmrt_realtime_play` と必ず揃えること。
pub const MAX_STANDBY_ERROR_BYTES: usize = 1024;

/// [`FastMidiCommand::FadeOutInstances`] の fadeout の長さとして受け付ける最大値（ミリ秒）。
pub const MAX_FADE_OUT_MS: u32 = 10_000;

pub type InstanceId = u8;
pub type TimelineId = u64;

/// live 出力バッファの倍率として受け付ける最大値。
///
/// サーバーはリングを `buffer_size * MAX_BUFFER_MULTIPLIER` フレームで確保するので、
/// ここを広げるときは `realtime-play-server` 側の `MAX_BUFFER_MULTIPLIER` も揃えること。
/// 倍率は wire format 上 `u32` なので、この定数を上げても共有メモリのレイアウトは
/// 変わらない（`windows/protocol.rs` の `VERSION` は据え置きでよい）。
pub const MAX_BUFFER_MULTIPLIER: u16 = 256;

/// 倍率として受け付ける値か（1〜[`MAX_BUFFER_MULTIPLIER`] の2冪）。
pub fn is_valid_buffer_multiplier(multiplier: u16) -> bool {
    multiplier.is_power_of_two() && multiplier <= MAX_BUFFER_MULTIPLIER
}

pub fn validate_instance_id(instance_id: InstanceId) -> Result<(), FastIpcError> {
    if usize::from(instance_id) >= INSTANCE_COUNT {
        return Err(FastIpcError::InvalidInstance {
            instance_id,
            count: INSTANCE_COUNT,
        });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FastMidiEvent {
    pub instance_id: InstanceId,
    pub offset_frames: u32,
    pub message: [u8; 3],
}

/// A MIDI message at an absolute time from the beginning of one live timeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineMidiEvent {
    pub timeline_id: TimelineId,
    pub instance_id: InstanceId,
    pub timeline_seconds: f64,
    pub message: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveTimelineConfig {
    pub timeline_id: TimelineId,
    pub sample_rate_hz: f64,
    pub tempo_bpm: f64,
    pub time_signature_numerator: u16,
    pub time_signature_denominator: u16,
}

/// live timeline の tempo map へ積む変化点。
///
/// テンポは timeline の属性ではなく timeline 上のデータなので、これを送っても
/// timeline は作り直されない（[`FastMidiCommand::BeginLiveTimeline`] と違い、
/// プラグインの状態もサンプルクロックの原点も動かない）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveTempoChange {
    pub timeline_id: TimelineId,
    /// この絶対秒（timeline 原点から）から新しいテンポにする。
    pub at_seconds: f64,
    pub tempo_bpm: f64,
    pub time_signature_numerator: u16,
    pub time_signature_denominator: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimingMetrics {
    pub events: u64,
    pub late_events: u64,
    pub late_events_total: u64,
    pub max_late_samples: u64,
    pub max_late_us: f64,
    pub output_lead_min_frames: u64,
    pub output_lead_max_frames: u64,
    pub process_load_p95: f32,
    pub process_load_max: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LimiterMeter {
    pub current_reduction_db: f32,
    pub peak_reduction_db: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FastMidiCommand {
    Midi {
        events: Vec<FastMidiEvent>,
    },
    BeginLiveTimeline(LiveTimelineConfig),
    /// 走っている timeline の tempo map へテンポ変化点を積む。timeline は作り直さない。
    SetLiveTempo(LiveTempoChange),
    TimelineMidi {
        events: Vec<TimelineMidiEvent>,
    },
    PreparePatch {
        request_id: u32,
        instance_id: InstanceId,
        patch: Option<String>,
        /// instance の出力に掛ける effect chain（`"effects after instrument"` の値の JSON 文字列）。
        /// 空なら chain 無し。
        effect_chain: String,
        probe: bool,
    },
    /// 非演奏 bank への先読みロード。
    ///
    /// [`FastMidiCommand::PreparePatch`] と違い、クライアントが
    /// 「この instance は今まさに鳴らしている bank には属さない」と宣言している。
    /// サーバーはこれを根拠に、その bank のレンダーを止めてロードしてよい。
    /// 現在 bank の行音色変更・MML overlay・起動時 prepare は宣言できないので
    /// 従来どおり [`FastMidiCommand::PreparePatch`] を使う。
    PrepareStandbyPatch {
        request_id: u32,
        instance_id: InstanceId,
        patch: Option<String>,
        /// [`FastMidiCommand::PreparePatch`] の `effect_chain` と同じ。
        effect_chain: String,
    },
    SetBufferMultiplier {
        multiplier: u16,
    },
    /// live mix で instance へ掛ける振幅ゲイン。千分率（1000 = 等倍）で運ぶ。
    SetInstanceGain {
        instance_id: InstanceId,
        gain_milli: u32,
    },
    /// live mixのinstance別RMS auto-trimを切り替える。
    SetAutoGain {
        enabled: bool,
    },
    Stop {
        instance_id: InstanceId,
    },
    StopAll,
    /// 指定した live instance の出力を、今の音量から 0 まで `fade_ms` ミリ秒で絞る。
    ///
    /// 0 に達した instance は、鳴っている voice と effect chain の余韻を捨て、次の音は
    /// 等倍から鳴る。timeline も他の instance も触らない。
    FadeOutInstances {
        instance_ids: Vec<InstanceId>,
        fade_ms: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FastIpcError {
    UnsupportedPlatform,
    NotAvailable,
    AlreadyConnected,
    ProtocolMismatch,
    ServerStopped,
    QueueFull,
    ResponseTimeout,
    RequestFailed(String),
    TooManyMidiMessages {
        count: usize,
        max: usize,
    },
    PatchTooLong {
        bytes: usize,
        max: usize,
    },
    EffectChainTooLong {
        bytes: usize,
        max: usize,
    },
    ResponseTooLong {
        bytes: usize,
        max: usize,
    },
    InvalidInstance {
        instance_id: InstanceId,
        count: usize,
    },
    InvalidPayload(String),
    Os {
        operation: &'static str,
        code: u32,
    },
}

impl fmt::Display for FastIpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                write!(f, "shared-memory MIDI is only supported on Windows")
            }
            Self::NotAvailable => write!(f, "shared-memory MIDI server is not available"),
            Self::AlreadyConnected => write!(f, "another shared-memory MIDI client is connected"),
            Self::ProtocolMismatch => write!(f, "shared-memory MIDI protocol mismatch"),
            Self::ServerStopped => write!(f, "shared-memory MIDI server stopped responding"),
            Self::QueueFull => write!(f, "shared-memory MIDI queue is full"),
            Self::ResponseTimeout => write!(f, "shared-memory MIDI response timed out"),
            Self::RequestFailed(message) => write!(f, "shared-memory request failed: {message}"),
            Self::TooManyMidiMessages { count, max } => {
                write!(f, "too many MIDI messages ({count}; max {max})")
            }
            Self::PatchTooLong { bytes, max } => {
                write!(f, "patch path is too long ({bytes} bytes; max {max})")
            }
            Self::EffectChainTooLong { bytes, max } => {
                write!(f, "effect chain is too long ({bytes} bytes; max {max})")
            }
            Self::ResponseTooLong { bytes, max } => {
                write!(f, "response is too long ({bytes} bytes; max {max})")
            }
            Self::InvalidInstance { instance_id, count } => {
                write!(f, "instance {instance_id} is outside 0..{count}")
            }
            Self::InvalidPayload(message) => write!(f, "invalid shared-memory payload: {message}"),
            Self::Os { operation, code } => {
                write!(f, "{operation} failed with Windows error {code}")
            }
        }
    }
}

impl std::error::Error for FastIpcError {}

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{FastMidiClient, FastMidiServer};

#[cfg(all(test, windows))]
mod windows_tests;

#[cfg(not(windows))]
mod unsupported;

#[cfg(not(windows))]
pub use unsupported::{FastMidiClient, FastMidiServer};

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    const TEST_PORT: u16 = 12_345;

    #[test]
    fn unsupported_platform_returns_explicit_error() {
        assert!(matches!(
            FastMidiClient::connect(TEST_PORT),
            Err(FastIpcError::UnsupportedPlatform)
        ));
        assert!(matches!(
            FastMidiServer::create(TEST_PORT),
            Err(FastIpcError::UnsupportedPlatform)
        ));
    }
}
