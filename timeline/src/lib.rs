//! Audio-host timeline primitives and deterministic block scheduling.
//!
//! This crate deliberately knows nothing about CLAP, an audio device, IPC, threads, or wall-clock
//! time. Callers describe musical events in absolute seconds; conversion to the discrete sample
//! lattice happens once, at the rendering boundary.

mod diagnostics;
mod scheduler;
mod transport;
mod value;

pub use diagnostics::{PlacementObservation, TimingDiagnostics, TimingSnapshot};
pub use scheduler::{
    BlockEvent, BlockEvents, BlockScheduler, LateEventPolicy, QuantizedTime, Timed,
};
pub use transport::{
    ConstantTempoTimeline, FreeRunningTimeline, TransportSnapshot, TransportTimeline,
};
pub use value::{BlockSpan, SamplePosition, SampleRate, TimelineError, TimelineSeconds};

#[cfg(test)]
mod tests;
