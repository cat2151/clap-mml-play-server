use anyhow::Result;
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

/// SMF が何も指定しないときのテンポ（μs/拍）＝ BPM120。
const DEFAULT_TEMPO_US: f64 = 500_000.0;
const DEFAULT_NUMERATOR: u16 = 4;
const DEFAULT_DENOMINATOR: u16 = 4;
/// SMF の拍子は分母を2の冪指数で持つ。異常値で `1 << n` を溢れさせないための上限。
const MAX_DENOMINATOR_POW2: u8 = 15;

/// サンプル単位のタイムスタンプを持つ生MIDIイベント
#[derive(Debug, Clone)]
pub struct TimedMidiEvent {
    /// 何サンプル目に発火するか
    pub sample_pos: u64,
    pub message: MidiEvent,
}

#[derive(Debug, Clone)]
pub enum MidiEvent {
    NoteOn { channel: u8, key: u8, velocity: u8 },
    NoteOff { channel: u8, key: u8, velocity: u8 },
}

/// tempo map の1区間（絶対秒）。
///
/// MML の `tNNN` は SMF の tempo meta になるので、曲中でテンポが変われば区間が増える。
/// プラグインへ渡す CLAP transport はここから組む。これを渡さないと、tempo-sync する
/// パッチ（delay sync / LFO sync）がオフラインとリアルタイムで違う音になる。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmfTempoChange {
    pub at_seconds: f64,
    pub tempo_bpm: f64,
    pub numerator: u16,
    pub denominator: u16,
}

/// SMF を読んだ結果。イベント列・総サンプル数・tempo map。
#[derive(Debug, Clone)]
pub struct SmfPlayback {
    pub events: Vec<TimedMidiEvent>,
    pub total_samples: u64,
    /// 0秒から始まる区間の列。必ず1件以上あり、`at_seconds` は単調増加。
    pub tempo_map: Vec<SmfTempoChange>,
}

/// SMFファイルを読み、サンプル単位のイベント列と総サンプル数を返す
#[allow(dead_code)]
pub fn parse_smf(path: &str, sample_rate: f64) -> Result<SmfPlayback> {
    let raw = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("MIDIファイルが読めない ({}): {}", path, e))?;
    parse_smf_playback(&raw, sample_rate)
}

/// tempo map の要らない呼び出し元向けの窓口。イベント列と総サンプル数だけを返す。
pub fn parse_smf_bytes(raw: &[u8], sample_rate: f64) -> Result<(Vec<TimedMidiEvent>, u64)> {
    let playback = parse_smf_playback(raw, sample_rate)?;
    Ok((playback.events, playback.total_samples))
}

/// SMFバイト列をメモリ上でパースする（TUIパイプライン用）
///
/// tick→秒の変換は tempo map を引く区分線形。テンポ変化点より後ろを一律に新しい
/// テンポで割り直すと、変化点までの経過時間まで書き換わって曲全体がずれる。
pub fn parse_smf_playback(raw: &[u8], sample_rate: f64) -> Result<SmfPlayback> {
    let smf = Smf::parse(raw)?;

    let ticks_per_beat = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as f64,
        Timing::Timecode(_, _) => anyhow::bail!("Timecodeベースのタイミングは未対応"),
    };

    let tempo = TempoMapTicks::build(&smf, ticks_per_beat);
    let mut events: Vec<TimedMidiEvent> = Vec::new();
    let mut max_sample: u64 = 0;

    for track in &smf.tracks {
        let mut tick: u64 = 0;

        for event in track {
            tick += event.delta.as_int() as u64;
            let sample_pos = (tempo.seconds_at(tick) * sample_rate) as u64;

            if sample_pos > max_sample {
                max_sample = sample_pos;
            }

            let TrackEventKind::Midi { channel, message } = event.kind else {
                continue;
            };
            let ch = channel.as_int();
            match message {
                MidiMessage::NoteOn { key, vel } => {
                    let velocity = vel.as_int();
                    let msg = if velocity == 0 {
                        MidiEvent::NoteOff {
                            channel: ch,
                            key: key.as_int(),
                            velocity: 0,
                        }
                    } else {
                        MidiEvent::NoteOn {
                            channel: ch,
                            key: key.as_int(),
                            velocity,
                        }
                    };
                    events.push(TimedMidiEvent {
                        sample_pos,
                        message: msg,
                    });
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
    }

    let tail = (sample_rate * 2.0) as u64;
    events.sort_by_key(|e| e.sample_pos);

    Ok(SmfPlayback {
        events,
        total_samples: max_sample + tail,
        tempo_map: tempo.to_tempo_map(),
    })
}

#[derive(Clone, Copy)]
enum MetaChange {
    Tempo(f64),
    TimeSignature(u16, u16),
}

/// tick 領域の tempo map。区間の頭で「そこまでの経過秒」を確定させて持つので、
/// `seconds_at` は1区間ぶんの掛け算だけで済み、誤差も積もらない。
struct TempoMapTicks {
    /// (区間の開始 tick, 開始秒, テンポ μs/拍, 拍子分子, 拍子分母)
    segments: Vec<(u64, f64, f64, u16, u16)>,
    ticks_per_beat: f64,
}

impl TempoMapTicks {
    fn build(smf: &Smf, ticks_per_beat: f64) -> Self {
        let mut map = Self {
            segments: vec![(
                0,
                0.0,
                DEFAULT_TEMPO_US,
                DEFAULT_NUMERATOR,
                DEFAULT_DENOMINATOR,
            )],
            ticks_per_beat,
        };
        for (tick, change) in collect_meta_changes(smf) {
            map.push(tick, change);
        }
        map
    }

    fn push(&mut self, tick: u64, change: MetaChange) {
        let last = *self.segments.last().expect("tempo map is never empty");
        // 走査は tick 昇順。同じ tick への追記は末尾を書き換える（区間を増やさない）。
        let seconds = self.seconds_at(tick);
        let mut next = (tick, seconds, last.2, last.3, last.4);
        match change {
            MetaChange::Tempo(tempo_us) => next.2 = tempo_us,
            MetaChange::TimeSignature(numerator, denominator) => {
                next.3 = numerator;
                next.4 = denominator;
            }
        }
        if (next.2, next.3, next.4) == (last.2, last.3, last.4) {
            return;
        }
        if tick == last.0 {
            *self.segments.last_mut().expect("tempo map is never empty") = next;
        } else {
            self.segments.push(next);
        }
    }

    fn seconds_at(&self, tick: u64) -> f64 {
        let segment = self
            .segments
            .iter()
            .rev()
            .find(|segment| segment.0 <= tick)
            .expect("the first segment starts at tick 0");
        segment.1 + (tick - segment.0) as f64 * segment.2 / (self.ticks_per_beat * 1_000_000.0)
    }

    fn to_tempo_map(&self) -> Vec<SmfTempoChange> {
        self.segments
            .iter()
            .map(
                |&(_, at_seconds, tempo_us, numerator, denominator)| SmfTempoChange {
                    at_seconds,
                    tempo_bpm: 60_000_000.0 / tempo_us,
                    numerator,
                    denominator,
                },
            )
            .collect()
    }
}

/// 全トラックの tempo / 拍子メタを1本の tick 軸へ集める。
///
/// SMF format 1 ではふつう track 0 だけが持つが、規格上はどのトラックにも置ける。
/// 同じ tick に複数あれば元のトラック順・イベント順で後勝ちになる（安定ソート）。
fn collect_meta_changes(smf: &Smf) -> Vec<(u64, MetaChange)> {
    let mut changes = Vec::new();
    for track in &smf.tracks {
        let mut tick: u64 = 0;
        for event in track {
            tick += event.delta.as_int() as u64;
            match event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(tempo_us)) => {
                    changes.push((tick, MetaChange::Tempo(tempo_us.as_int() as f64)));
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(
                    numerator,
                    denominator_pow2,
                    _,
                    _,
                )) => {
                    changes.push((
                        tick,
                        MetaChange::TimeSignature(
                            u16::from(numerator.max(1)),
                            1u16 << denominator_pow2.min(MAX_DENOMINATOR_POW2),
                        ),
                    ));
                }
                _ => {}
            }
        }
    }
    changes.sort_by_key(|(tick, _)| *tick);
    changes
}

#[cfg(test)]
#[path = "midi_tests.rs"]
mod tests;
