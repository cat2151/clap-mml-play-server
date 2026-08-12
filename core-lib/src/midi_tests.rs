use super::*;

/// フォーマット 0、1 トラック、120 ticks/beat のヘッダだけ（イベントなし）の最小 SMF
fn minimal_smf_bytes() -> Vec<u8> {
    vec![
        // MThd
        0x4D, 0x54, 0x68, 0x64, // header data length = 6
        0x00, 0x00, 0x00, 0x06, // format 0
        0x00, 0x00, // 1 track
        0x00, 0x01, // 120 ticks per beat
        0x00, 0x78, // MTrk
        0x4D, 0x54, 0x72, 0x6B, // track data length = 4
        0x00, 0x00, 0x00, 0x04, // delta=0, end-of-track meta
        0x00, 0xFF, 0x2F, 0x00,
    ]
}

/// NoteOn (ch0 key=60 vel=64) + NoteOff (delta=480 ticks) を含む SMF
fn smf_with_note() -> Vec<u8> {
    // track data:
    //   delta=0, NoteOn ch0 key=60 vel=64   : 00 90 3C 40  (4 bytes)
    //   delta=480, NoteOff ch0 key=60 vel=0  : 83 60 80 3C 00 (5 bytes)
    //   delta=0, end-of-track               : 00 FF 2F 00  (4 bytes)
    // total track data = 13 bytes
    vec![
        // MThd
        0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x78, // 120 ticks per beat
        // MTrk
        0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x0D, // track data length = 13
        // NoteOn: delta=0
        0x00, 0x90, 0x3C, 0x40,
        // NoteOff: delta=480 (var-len: 0x83 0x60), ch0 key=60 vel=0
        0x83, 0x60, 0x80, 0x3C, 0x00, // end-of-track: delta=0
        0x00, 0xFF, 0x2F, 0x00,
    ]
}

/// NoteOn で velocity=0 のイベントを含む SMF（NoteOff として扱われるべき）
fn smf_with_noteon_vel_zero() -> Vec<u8> {
    // track data:
    //   delta=0, NoteOn ch0 key=60 vel=0 : 00 90 3C 00  (4 bytes)
    //   delta=0, end-of-track            : 00 FF 2F 00  (4 bytes)
    // total = 8 bytes
    vec![
        0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x78, 0x4D,
        0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x08, 0x00, 0x90, 0x3C, 0x00, 0x00, 0xFF, 0x2F, 0x00,
    ]
}

#[test]
fn parse_smf_bytes_empty_track_returns_no_events() {
    let playback = parse_smf_playback(&minimal_smf_bytes(), 44100.0).unwrap();
    let (events, total_samples) = (playback.events, playback.total_samples);
    assert!(events.is_empty());
    // tail のみ: (44100 * 2) = 88200
    assert_eq!(total_samples, 88200);
}

#[test]
fn parse_smf_bytes_with_note_returns_two_events() {
    let events = parse_smf_playback(&smf_with_note(), 44100.0)
        .unwrap()
        .events;
    assert_eq!(events.len(), 2);
}

#[test]
fn parse_smf_bytes_first_event_is_noteon() {
    let events = parse_smf_playback(&smf_with_note(), 44100.0)
        .unwrap()
        .events;
    assert_eq!(events[0].sample_pos, 0);
    match events[0].message {
        MidiEvent::NoteOn {
            channel,
            key,
            velocity,
        } => {
            assert_eq!(channel, 0);
            assert_eq!(key, 60);
            assert_eq!(velocity, 64);
        }
        _ => panic!("最初のイベントは NoteOn であるべき"),
    }
}

#[test]
fn parse_smf_bytes_second_event_is_noteoff() {
    let events = parse_smf_playback(&smf_with_note(), 44100.0)
        .unwrap()
        .events;
    // delta=480 ticks, tempo=500000 µs/beat, tpb=120
    // secs = (480 * 500000) / (120 * 1_000_000) = 2.0 s
    // sample_pos = 2.0 * 44100 = 88200
    assert_eq!(events[1].sample_pos, 88200);
    match events[1].message {
        MidiEvent::NoteOff {
            channel,
            key,
            velocity,
        } => {
            assert_eq!(channel, 0);
            assert_eq!(key, 60);
            assert_eq!(velocity, 0);
        }
        _ => panic!("2番目のイベントは NoteOff であるべき"),
    }
}

#[test]
fn parse_smf_bytes_total_samples_includes_tail() {
    let total_samples = parse_smf_playback(&smf_with_note(), 44100.0)
        .unwrap()
        .total_samples;
    // max_sample = 88200, tail = 88200 → 合計 176400
    let tail = (44100.0 * 2.0) as u64;
    assert_eq!(total_samples, 88200 + tail);
}

#[test]
fn parse_smf_bytes_noteon_vel_zero_treated_as_noteoff() {
    let events = parse_smf_playback(&smf_with_noteon_vel_zero(), 44100.0)
        .unwrap()
        .events;
    assert_eq!(events.len(), 1);
    match events[0].message {
        MidiEvent::NoteOff { key, .. } => {
            assert_eq!(key, 60);
        }
        _ => panic!("velocity=0 の NoteOn は NoteOff として扱われるべき"),
    }
}

/// 4拍目でテンポが半分になる SMF。120 ticks/beat。
///
/// tick 480 (= 4拍) まで BPM120、そこから BPM60。tick 960 の NoteOff は
/// 区分線形なら 2.0 + 4.0 = 6.0 秒。テンポ変化点より後ろを一律に新テンポで
/// 割り直すと 8.0 秒になる。
fn smf_with_tempo_change() -> Vec<u8> {
    use midly::{Format, Header, MetaMessage, Smf, TrackEvent};

    let track = vec![
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::TimeSignature(3, 2, 24, 8)),
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(500_000.into())),
        },
        TrackEvent {
            delta: 480.into(),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(1_000_000.into())),
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOn {
                    key: 60.into(),
                    vel: 100.into(),
                },
            },
        },
        TrackEvent {
            delta: 480.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOff {
                    key: 60.into(),
                    vel: 0.into(),
                },
            },
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ];
    let smf = Smf {
        header: Header::new(Format::SingleTrack, Timing::Metrical(120.into())),
        tracks: vec![track],
    };
    let mut bytes = Vec::new();
    smf.write_std(&mut bytes).unwrap();
    bytes
}

#[test]
fn tempo_changes_are_reported_as_a_piecewise_map() {
    let playback = parse_smf_playback(&smf_with_tempo_change(), 1_000.0).unwrap();

    assert_eq!(
        playback.tempo_map,
        vec![
            SmfTempoChange {
                at_seconds: 0.0,
                tempo_bpm: 120.0,
                numerator: 3,
                denominator: 4,
            },
            SmfTempoChange {
                at_seconds: 2.0,
                tempo_bpm: 60.0,
                numerator: 3,
                denominator: 4,
            },
        ]
    );
}

/// テンポ変化より後ろの位置は、変化点までの経過時間を保ったまま新テンポで進むこと。
#[test]
fn events_after_a_tempo_change_keep_the_elapsed_time_before_it() {
    let playback = parse_smf_playback(&smf_with_tempo_change(), 1_000.0).unwrap();

    assert_eq!(playback.events[0].sample_pos, 2_000, "NoteOn は 2.0 秒");
    assert_eq!(
        playback.events[1].sample_pos, 6_000,
        "NoteOff は 2.0 + 4.0 = 6.0 秒（8.0 秒なら変化点前まで割り直している）"
    );
}

#[test]
fn a_file_without_any_tempo_meta_falls_back_to_120_bpm_in_four_four() {
    let playback = parse_smf_playback(&smf_with_note(), 44_100.0).unwrap();

    assert_eq!(
        playback.tempo_map,
        vec![SmfTempoChange {
            at_seconds: 0.0,
            tempo_bpm: 120.0,
            numerator: 4,
            denominator: 4,
        }]
    );
}

#[test]
fn parse_smf_bytes_invalid_returns_error() {
    let invalid = b"not a midi file";
    let result = parse_smf_playback(invalid, 44100.0);
    assert!(result.is_err());
}
