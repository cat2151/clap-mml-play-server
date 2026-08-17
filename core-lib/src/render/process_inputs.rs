//! プラグインへ渡す `process()` 入力を、広告された能力に合わせて組む。
//!
//! Surge XT と Dexed の差（audio input を持つか、note を CLAP と MIDI のどちらで
//! 受け取るか）が出るのはここだけで、レンダリングループ本体（[`super`]）は
//! 能力を判断せずこの 2 つを呼ぶ。

use clack_host::events::event_types::{MidiEvent as ClapMidiEvent, NoteOffEvent, NoteOnEvent};
use clack_host::events::Match;
use clack_host::prelude::*;

use super::descriptor::{NoteEventDialect, PluginCapabilities};
use crate::midi::MidiEvent;

/// audio input port を持つプラグインにだけ入力チャンネルを確保する。
///
/// Dexed のように input 0 のプラグインへは buffer を 1 本も渡さないので、
/// ここで確保してしまうと使われないメモリが残るだけになる。
pub(super) fn input_buffer(capabilities: &PluginCapabilities, buffer_size: usize) -> Vec<f32> {
    if capabilities.audio_input_ports == 0 {
        Vec::new()
    } else {
        vec![0.0; buffer_size]
    }
}

/// オフライン経路の note を、プラグインが広告した方言で input event buffer へ積む。
///
/// `EventFlags::IS_LIVE` は live 経路専用。オフラインレンダリングの再現性に関わるので
/// ここでは付けない。
pub(super) fn push_offline_note_event(
    events: &mut EventBuffer,
    offset: u32,
    message: MidiEvent,
    dialect: NoteEventDialect,
) {
    match dialect {
        NoteEventDialect::Clap => match message {
            MidiEvent::NoteOn {
                channel,
                key,
                velocity,
            } => events.push(&NoteOnEvent::new(
                offset,
                Pckn::new(0u16, channel as u16, key as u16, Match::All),
                velocity as f64 / 127.0,
            )),
            MidiEvent::NoteOff {
                channel,
                key,
                velocity,
            } => events.push(&NoteOffEvent::new(
                offset,
                Pckn::new(0u16, channel as u16, key as u16, Match::All),
                velocity as f64 / 127.0,
            )),
        },
        NoteEventDialect::Midi => {
            events.push(&ClapMidiEvent::new(offset, 0, message.to_short_message()))
        }
    }
}
