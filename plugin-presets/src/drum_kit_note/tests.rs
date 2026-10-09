use super::*;

#[test]
fn higher_source_replaces_lower_and_duplicates_collapse_in_order() {
    let mut names = NoteNames::default();
    names.insert(36, NameSource::Sample, "kick_v1");
    names.insert(36, NameSource::GroupLabel, "Kick");
    names.insert(36, NameSource::Sample, "kick_v2");
    names.insert(38, NameSource::Sample, "snare rr1");
    names.insert(38, NameSource::Sample, "snare rr2");
    names.insert(38, NameSource::Sample, "snare rr1");
    names.insert(38, NameSource::Sample, "snare rr3");
    names.insert_note(40);
    names.insert(41, NameSource::RegionLabel, "  ");
    names.insert(42, NameSource::KeyLabel, "\"Closed Hat\"");
    assert_eq!(
        names.finish(),
        vec![
            DrumKitNote {
                note: 36,
                name: Some("Kick".to_string())
            },
            DrumKitNote {
                note: 38,
                name: Some("snare rr1 +2".to_string())
            },
            DrumKitNote {
                note: 40,
                name: None
            },
            DrumKitNote {
                note: 41,
                name: None
            },
            DrumKitNote {
                note: 42,
                name: Some("Closed Hat".to_string())
            },
        ]
    );
}

#[test]
fn sample_name_drops_directories_and_extension_but_keeps_synthetic_waves() {
    assert_eq!(
        sample_name(r"Samples\Taiko Drum Hit 1-15.flac"),
        "Taiko Drum Hit 1-15"
    );
    assert_eq!(sample_name("../kit/909 closed hat.wav"), "909 closed hat");
    assert_eq!(sample_name("*sine"), "*sine");
    assert_eq!(sample_name(".hidden"), ".hidden");
    assert_eq!(sample_name("201 AfrLogDrm$GEXT2"), "201 AfrLogDrm");
    assert_eq!(sample_name("$KICK"), "$KICK");
}
