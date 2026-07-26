//! Low-latency MIDI IPC used by clap-mml-render-tui and the realtime play server.

use std::fmt;

/// 1スロットに詰められる MIDI メッセージ数。grid sequencer の1ステップは
/// note off 16 + retrigger 込みの note on 32 まで膨らむため、32 では足りない。
pub const MAX_MIDI_MESSAGES: usize = 128;
pub const MAX_PATCH_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FastMidiCommand {
    Midi {
        messages: Vec<[u8; 3]>,
        /// 各メッセージの発音位置（現在の live 位置からのフレーム数）。`messages` と同数。
        offsets: Vec<u32>,
        patch: Option<String>,
    },
    SetBufferMultiplier {
        multiplier: u8,
    },
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FastIpcError {
    UnsupportedPlatform,
    NotAvailable,
    AlreadyConnected,
    ProtocolMismatch,
    ServerStopped,
    QueueFull,
    TooManyMidiMessages { count: usize, max: usize },
    PatchTooLong { bytes: usize, max: usize },
    InvalidPayload(String),
    Os { operation: &'static str, code: u32 },
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
            Self::TooManyMidiMessages { count, max } => {
                write!(f, "too many MIDI messages ({count}; max {max})")
            }
            Self::PatchTooLong { bytes, max } => {
                write!(f, "patch path is too long ({bytes} bytes; max {max})")
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

#[cfg(not(windows))]
mod unsupported {
    use super::*;
    use std::time::Duration;

    pub struct FastMidiClient;

    impl FastMidiClient {
        pub fn connect(_port: u16) -> Result<Self, FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn send_midi(
            &mut self,
            _messages: &[[u8; 3]],
            _patch: Option<&str>,
        ) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn send_midi_with_offsets(
            &mut self,
            _events: &[(u32, [u8; 3])],
            _patch: Option<&str>,
        ) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn stop(&mut self) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
        }

        pub fn set_buffer_multiplier(&mut self, _multiplier: u8) -> Result<(), FastIpcError> {
            Err(FastIpcError::UnsupportedPlatform)
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
