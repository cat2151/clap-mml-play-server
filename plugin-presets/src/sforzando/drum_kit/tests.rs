use super::*;

fn kit(name: &str, sfz: &str) -> bool {
    let dir = std::env::temp_dir().join(format!("cmrt_test_sfz_drum_kit_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("program.sfz");
    std::fs::write(&path, sfz).unwrap();
    sfz_is_drum_kit(&path).unwrap()
}

/// `(key, sample)` を 1 鍵 1 region で並べる。
fn one_key_regions(samples: &[(u8, &str)]) -> String {
    samples
        .iter()
        .map(|(key, sample)| format!("<region> sample={sample} key={key}\n"))
        .collect()
}

#[test]
fn different_sounds_per_key_are_a_kit() {
    let sfz = one_key_regions(&[
        (36, "Kick_v1_rr1.wav"),
        (38, "Snare_v1_rr1.wav"),
        (42, "Closed Hat.wav"),
        (46, "Open Hat.wav"),
    ]);

    assert!(kit("drums", &sfz));
}

#[test]
fn chromatic_samples_named_by_note_are_not_a_kit() {
    let sfz = one_key_regions(&[
        (60, "Samples/Violin/4_C-PB-loop.wav"),
        (61, "Samples/Violin/4_Db-PB-loop.wav"),
        (62, "Samples/Violin/4_D-PB.wav"),
        (63, "Samples/Violin/4_Eb_f.wav"),
        (64, "Samples/Violin/E4_mf.wav"),
    ]);

    assert!(!kit("chromatic", &sfz));
}

#[test]
fn stretched_pitched_ranges_are_not_a_kit() {
    let sfz = "<region> sample=Hit AbMaj.flac lokey=0 hikey=47\n\
               <region> sample=Stab EMaj.flac lokey=48 hikey=59\n\
               <region> sample=Hit E7.flac lokey=60 hikey=127\n";

    assert!(!kit("ranges", sfz));
}

#[test]
fn key_pairs_that_do_not_track_pitch_are_a_kit_even_with_numbered_names() {
    let sfz = "<group> pitch_keytrack=0 lokey=c2 hikey=c#2 pitch_keycenter=c2\n\
               <region> sample=Taiko Drum Hit 1-15.flac\n\
               <group> pitch_keytrack=0 lokey=d2 hikey=d#2 pitch_keycenter=d2\n\
               <region> sample=Taiko Drum Hit 2-2.flac\n";

    assert!(kit("taiko", sfz));
}

#[test]
fn a_noise_layer_over_a_pitched_instrument_is_not_a_kit() {
    let sfz = "<region> sample=28GLfrE1.wav lokey=26 hikey=29 pitch_keycenter=28\n\
               <region> sample=31GLfrG1.wav lokey=30 hikey=32 pitch_keycenter=31\n\
               <group> pitch_keytrack=0\n\
               <region> sample=BsClick 1.wav lokey=26 hikey=29\n\
               <region> sample=BsClick 10.wav lokey=30 hikey=31\n";

    assert!(!kit("noise_layer", sfz));
}

#[test]
fn one_untracked_range_is_the_same_sound_on_every_key() {
    let sfz = "<global> lokey=b1 hikey=e5 pitch_keytrack=0 seq_length=2\n\
               <region> sample=Noise/1.flac seq_position=1\n\
               <region> sample=Noise/2.flac seq_position=2\n";

    assert!(!kit("one_range", sfz));
}

#[test]
fn built_in_waveforms_are_told_apart_by_their_synthesis_opcodes() {
    let sfz = "<group> sample=*sine transpose=1\n<region> key=36\n\
               <group> sample=*triangle pitch_keycenter=88\n<region> key=35\n\
               <group> sample=*noise ampeg_decay=0.2\n<region> key=38\n";

    assert!(kit("builtin", sfz));
}

#[test]
fn a_drum_section_beside_a_pitched_synth_range_is_a_kit() {
    let mut sfz = one_key_regions(&[
        (36, "Kick.flac"),
        (37, "Rim.flac"),
        (38, "Snare.flac"),
        (39, "Clap.flac"),
        (40, "Tom.flac"),
        (41, "Closed Hat.flac"),
        (42, "Open Hat.flac"),
        (43, "Crash.flac"),
    ]);
    sfz.push_str("<region> sample=Synth C4.flac lokey=64 hikey=127 pitch_keycenter=72\n");

    assert!(kit("mixed", &sfz));
}

#[test]
fn release_samples_do_not_count_as_keys() {
    let sfz = "<region> sample=Piano C4.wav lokey=0 hikey=127 pitch_keycenter=60\n\
               <region> sample=Pedal Up.wav key=21 trigger=release\n\
               <region> sample=Key Off.wav key=22 trigger=release\n";

    assert!(!kit("release", sfz));
}
