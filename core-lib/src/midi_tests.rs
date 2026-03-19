use super::*;

/// フォーマット 0、1 トラック、120 ticks/beat のヘッダだけ（イベントなし）の最小 SMF
fn minimal_smf_bytes() -> Vec<u8> {
    vec![
        // MThd
        0x4D, 0x54, 0x68, 0x64,
        // header data length = 6
        0x00, 0x00, 0x00, 0x06,
        // format 0
        0x00, 0x00,
        // 1 track
        0x00, 0x01,
        // 120 ticks per beat
        0x00, 0x78,
        // MTrk
        0x4D, 0x54, 0x72, 0x6B,
        // track data length = 4
        0x00, 0x00, 0x00, 0x04,
        // delta=0, end-of-track meta
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
        0x4D, 0x54, 0x68, 0x64,
        0x00, 0x00, 0x00, 0x06,
        0x00, 0x00,
        0x00, 0x01,
        0x00, 0x78, // 120 ticks per beat
        // MTrk
        0x4D, 0x54, 0x72, 0x6B,
        0x00, 0x00, 0x00, 0x0D, // track data length = 13
        // NoteOn: delta=0
        0x00, 0x90, 0x3C, 0x40,
        // NoteOff: delta=480 (var-len: 0x83 0x60), ch0 key=60 vel=0
        0x83, 0x60, 0x80, 0x3C, 0x00,
        // end-of-track: delta=0
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
        0x4D, 0x54, 0x68, 0x64,
        0x00, 0x00, 0x00, 0x06,
        0x00, 0x00,
        0x00, 0x01,
        0x00, 0x78,
        0x4D, 0x54, 0x72, 0x6B,
        0x00, 0x00, 0x00, 0x08,
        0x00, 0x90, 0x3C, 0x00,
        0x00, 0xFF, 0x2F, 0x00,
    ]
}

#[test]
fn parse_smf_bytes_empty_track_returns_no_events() {
    let (events, total_samples) = parse_smf_bytes(&minimal_smf_bytes(), 44100.0).unwrap();
    assert!(events.is_empty());
    // tail のみ: (44100 * 2) = 88200
    assert_eq!(total_samples, 88200);
}

#[test]
fn parse_smf_bytes_with_note_returns_two_events() {
    let (events, _) = parse_smf_bytes(&smf_with_note(), 44100.0).unwrap();
    assert_eq!(events.len(), 2);
}

#[test]
fn parse_smf_bytes_first_event_is_noteon() {
    let (events, _) = parse_smf_bytes(&smf_with_note(), 44100.0).unwrap();
    assert_eq!(events[0].sample_pos, 0);
    match events[0].message {
        MidiEvent::NoteOn { channel, key, velocity } => {
            assert_eq!(channel, 0);
            assert_eq!(key, 60);
            assert_eq!(velocity, 64);
        }
        _ => panic!("最初のイベントは NoteOn であるべき"),
    }
}

#[test]
fn parse_smf_bytes_second_event_is_noteoff() {
    let (events, _) = parse_smf_bytes(&smf_with_note(), 44100.0).unwrap();
    // delta=480 ticks, tempo=500000 µs/beat, tpb=120
    // secs = (480 * 500000) / (120 * 1_000_000) = 2.0 s
    // sample_pos = 2.0 * 44100 = 88200
    assert_eq!(events[1].sample_pos, 88200);
    match events[1].message {
        MidiEvent::NoteOff { channel, key, velocity } => {
            assert_eq!(channel, 0);
            assert_eq!(key, 60);
            assert_eq!(velocity, 0);
        }
        _ => panic!("2番目のイベントは NoteOff であるべき"),
    }
}

#[test]
fn parse_smf_bytes_total_samples_includes_tail() {
    let (_, total_samples) = parse_smf_bytes(&smf_with_note(), 44100.0).unwrap();
    // max_sample = 88200, tail = 88200 → 合計 176400
    let tail = (44100.0 * 2.0) as u64;
    assert_eq!(total_samples, 88200 + tail);
}

#[test]
fn parse_smf_bytes_noteon_vel_zero_treated_as_noteoff() {
    let (events, _) = parse_smf_bytes(&smf_with_noteon_vel_zero(), 44100.0).unwrap();
    assert_eq!(events.len(), 1);
    match events[0].message {
        MidiEvent::NoteOff { key, .. } => {
            assert_eq!(key, 60);
        }
        _ => panic!("velocity=0 の NoteOn は NoteOff として扱われるべき"),
    }
}

#[test]
fn parse_smf_bytes_invalid_returns_error() {
    let invalid = b"not a midi file";
    let result = parse_smf_bytes(invalid, 44100.0);
    assert!(result.is_err());
}
