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
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024;

pub type InstanceId = u8;

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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LimiterMeter {
    pub current_reduction_db: f32,
    pub peak_reduction_db: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FastMidiCommand {
    Midi {
        events: Vec<FastMidiEvent>,
    },
    PreparePatch {
        request_id: u32,
        instance_id: InstanceId,
        patch: Option<String>,
        probe: bool,
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
mod unsupported {
    use super::*;
    use std::time::Duration;

    pub struct FastMidiClient;

    impl FastMidiClient {
        pub fn connect(_port: u16) -> Result<Self, FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn send_events(&mut self, _events: &[FastMidiEvent]) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn prepare_patch(
            &mut self,
            _instance_id: InstanceId,
            _patch: Option<&str>,
        ) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn probe_patch(
            &mut self,
            _instance_id: InstanceId,
            _patch: Option<&str>,
        ) -> Result<Vec<u8>, FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn stop(&mut self, _instance_id: InstanceId) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn stop_all(&mut self) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn set_buffer_multiplier(&mut self, _multiplier: u16) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn set_auto_gain_enabled(&mut self, _enabled: bool) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn limiter_meter(&self) -> LimiterMeter {
            LimiterMeter::default()
        }

        pub fn underrun_frames(&self) -> u64 {
            0
        }

        pub fn auto_gain_db(&self) -> [f32; MAX_INSTANCE_COUNT] {
            [0.0; MAX_INSTANCE_COUNT]
        }
    }

    pub struct FastMidiServer;

    impl FastMidiServer {
        pub fn create(_port: u16) -> Result<Self, FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn recv_timeout(
            &mut self,
            _timeout: Duration,
        ) -> Result<Option<FastMidiCommand>, FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn complete_request(
            &self,
            _request_id: u32,
            _result: Result<&[u8], &str>,
        ) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn publish_limiter_meter(&self, _meter: LimiterMeter) {}

        pub fn publish_underrun_frames(&self, _frames: u64) {}

        pub fn publish_auto_gain_db(&self, _gains_db: &[f32]) {}
    }
}

#[cfg(not(windows))]
pub use unsupported::{FastMidiClient, FastMidiServer};

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_returns_explicit_error() {
        assert!(matches!(
            FastMidiClient::connect(62154),
            Err(FastIpcError::UnsupportedPlatform)
        ));
        assert!(matches!(
            FastMidiServer::create(62154),
            Err(FastIpcError::UnsupportedPlatform)
        ));
    }
}
