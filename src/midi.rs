use anyhow::Result;
use midly::{Smf, Timing, TrackEventKind, MidiMessage};

/// サンプル単位のタイムスタンプを持つ生MIDIイベント
#[derive(Debug, Clone)]
pub struct TimedMidiEvent {
    /// 何サンプル目に発火するか
    pub sample_pos: u64,
    pub message: MidiEvent,
}

#[derive(Debug, Clone)]
pub enum MidiEvent {
    NoteOn  { channel: u8, key: u8, velocity: u8 },
    NoteOff { channel: u8, key: u8, velocity: u8 },
}

/// SMFファイルを読み、サンプル単位のイベント列と総サンプル数を返す
#[allow(dead_code)]
pub fn parse_smf(path: &str, sample_rate: f64) -> Result<(Vec<TimedMidiEvent>, u64)> {
    let raw = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("MIDIファイルが読めない ({}): {}", path, e))?;
    parse_smf_bytes(&raw, sample_rate)
}

/// SMFバイト列をメモリ上でパースする（TUIパイプライン用）
pub fn parse_smf_bytes(raw: &[u8], sample_rate: f64) -> Result<(Vec<TimedMidiEvent>, u64)> {
    let smf = Smf::parse(raw)?;

    let tpb = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as f64,
        Timing::Timecode(_, _) => anyhow::bail!("Timecodeベースのタイミングは未対応"),
    };

    let tempo_us: f64 = 500_000.0;
    let mut events: Vec<TimedMidiEvent> = Vec::new();
    let mut max_sample: u64 = 0;

    for track in &smf.tracks {
        let mut tick: u64 = 0;
        let mut cur_tempo: f64 = tempo_us;

        for event in track {
            tick += event.delta.as_int() as u64;
            let secs = (tick as f64 * cur_tempo) / (tpb * 1_000_000.0);
            let sample_pos = (secs * sample_rate) as u64;

            if sample_pos > max_sample {
                max_sample = sample_pos;
            }

            match event.kind {
                TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) => {
                    cur_tempo = t.as_int() as f64;
                }
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } => {
                            let velocity = vel.as_int();
                            let msg = if velocity == 0 {
                                MidiEvent::NoteOff { channel: ch, key: key.as_int(), velocity: 0 }
                            } else {
                                MidiEvent::NoteOn  { channel: ch, key: key.as_int(), velocity }
                            };
                            events.push(TimedMidiEvent { sample_pos, message: msg });
                        }
                        MidiMessage::NoteOff { key, vel } => {
                            events.push(TimedMidiEvent {
                                sample_pos,
                                message: MidiEvent::NoteOff {
                                    channel: ch,
                                    key: key.as_int(),
                                    velocity: vel.as_int(),
                                },
                            });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    let tail = (sample_rate * 2.0) as u64;
    events.sort_by_key(|e| e.sample_pos);

    Ok((events, max_sample + tail))
}

#[cfg(test)]
#[path = "midi_tests.rs"]
mod tests;
