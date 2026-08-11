//! Adapter from [`cmrt_timeline`] block time to CLAP process timing.
//!
//! CLAP describes time at the process-block boundary. It deliberately does not schedule host
//! events across threads or IPC; the pure timeline crate owns that policy and this crate only
//! translates its result into CLAP's `steady_time` and transport representation.

use clack_host::events::event_types::{TransportEvent, TransportFlags};
use clack_host::events::{EventFlags, EventHeader};
use clack_host::utils::{BeatTime, SecondsTime};
use cmrt_timeline::{BlockSpan, SampleRate, TransportTimeline};

#[derive(Clone, Copy, Debug)]
pub struct ProcessBlockTiming {
    pub steady_time: u64,
    pub transport: Option<TransportEvent>,
}

pub fn process_block_timing(
    block: BlockSpan,
    sample_rate: SampleRate,
    timeline: &impl TransportTimeline,
) -> ProcessBlockTiming {
    let at = sample_rate.sample_to_seconds(block.start());
    let transport = timeline.snapshot_at(at).map(|snapshot| {
        let mut flags = TransportFlags::HAS_TEMPO
            | TransportFlags::HAS_BEATS_TIMELINE
            | TransportFlags::HAS_SECONDS_TIMELINE
            | TransportFlags::HAS_TIME_SIGNATURE;
        if snapshot.playing {
            flags |= TransportFlags::IS_PLAYING;
        }
        TransportEvent {
            header: EventHeader::new_core(0, EventFlags::empty()),
            flags,
            song_pos_beats: BeatTime::from_float(snapshot.song_beats),
            song_pos_seconds: SecondsTime::from_float(snapshot.song_seconds.get()),
            tempo: snapshot.tempo_bpm,
            tempo_inc: 0.0,
            loop_start_beats: BeatTime::from_int(0),
            loop_end_beats: BeatTime::from_int(0),
            loop_start_seconds: SecondsTime::from_int(0),
            loop_end_seconds: SecondsTime::from_int(0),
            bar_start: BeatTime::from_float(snapshot.bar_start_beats),
            bar_number: snapshot.bar_number,
            time_signature_numerator: snapshot.time_signature_numerator,
            time_signature_denominator: snapshot.time_signature_denominator,
        }
    });
    ProcessBlockTiming {
        steady_time: block.start().get(),
        transport,
    }
}

#[cfg(test)]
mod tests;
