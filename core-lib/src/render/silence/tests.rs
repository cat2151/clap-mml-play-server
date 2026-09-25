use super::*;

/// process 済みの note だけを、重なり数も含めて NoteOff にする。
#[test]
fn active_notes_are_released_without_midi_all_sound_off() {
    let mut active = ActiveNotes::default();
    active.record(&[
        LiveMidiEvent {
            offset_frames: 8,
            message: [0x90, 60, 100],
        },
        LiveMidiEvent {
            offset_frames: 9,
            message: [0x90, 60, 110],
        },
        LiveMidiEvent {
            offset_frames: 10,
            message: [0x91, 67, 100],
        },
    ]);

    let events = active.note_off_events();

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].message, [0x80, 60, 0]);
    assert_eq!(events[1].message, [0x80, 60, 0]);
    assert_eq!(events[2].message, [0x81, 67, 0]);
    assert!(events.iter().all(|event| event.message[0] & 0xf0 == 0x80));
}

/// NoteOff を process したら台帳から消える。
#[test]
fn note_off_updates_the_active_note_ledger() {
    let mut active = ActiveNotes::default();
    active.record(&[LiveMidiEvent {
        offset_frames: 0,
        message: [0x90, 60, 100],
    }]);
    let note_offs = active.note_off_events();

    active.record(&note_offs);

    assert!(active.note_off_events().is_empty());
}

/// 張り直しの NoteOff は、その block の新しいイベントより前に置く。後ろに置くと、
/// 同じ block で鳴らし直した同じ note まで離してしまう。
#[test]
fn pending_release_puts_note_offs_before_the_block_events() {
    let mut active = ActiveNotes::default();
    active.record(&[LiveMidiEvent {
        offset_frames: 0,
        message: [0x90, 60, 100],
    }]);
    let new_note = LiveMidiEvent {
        offset_frames: 0,
        message: [0x90, 60, 90],
    };

    let events = active.note_offs_before(&[new_note]);

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].message, [0x80, 60, 0]);
    assert_eq!(events[1], new_note);
}
