use super::*;

/// 全 16 channel に CC120 と CC123 を出す。1 channel でも欠けるとそこが鳴り残る。
#[test]
fn every_channel_gets_all_sound_off_and_all_notes_off() {
    let events = all_sound_off_events();

    assert_eq!(events.len(), usize::from(MIDI_CHANNEL_COUNT) * 2);
    for channel in 0..MIDI_CHANNEL_COUNT {
        assert!(events
            .iter()
            .any(|event| event.message == [MIDI_CONTROL_CHANGE | channel, ALL_SOUND_OFF, 0]));
        assert!(events
            .iter()
            .any(|event| event.message == [MIDI_CONTROL_CHANGE | channel, ALL_NOTES_OFF, 0]));
    }
}

/// ブロックの先頭で切る。遅らせると切った直後の音が残る。
#[test]
fn everything_is_sent_at_the_start_of_the_block() {
    assert!(all_sound_off_events()
        .iter()
        .all(|event| event.offset_frames == 0));
}
