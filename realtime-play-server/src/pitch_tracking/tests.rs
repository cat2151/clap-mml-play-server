use super::*;

fn tone(frequency: f64) -> Vec<f32> {
    (0..24_000)
        .flat_map(|i| {
            let phase = std::f64::consts::TAU * frequency * i as f64 / 48_000.0;
            let value = (phase.sin() + 0.3 * (phase * 2.0).sin()) as f32 * 0.2;
            [value, value]
        })
        .collect()
}

#[test]
fn harmonic_pitch_tracks_semitones_and_octaves_but_fixed_tones_do_not() {
    let moving: Vec<_> = NOTES
        .into_iter()
        .map(|note| {
            observe(
                note,
                &tone(130.8128 * 2.0_f64.powf(f64::from(note - 48) / 12.0)),
            )
        })
        .collect();
    assert!(tracks_keyboard(&moving), "{moving:?}");
    let fixed: Vec<_> = NOTES
        .into_iter()
        .map(|note| observe(note, &tone(130.8128)))
        .collect();
    assert!(!tracks_keyboard(&fixed));
}

#[test]
fn silence_noise_and_missing_notes_do_not_imply_pitch_tracking() {
    assert!(observe(48, &vec![0.0; 48_000]).frequency_hz.is_none());
    let mut state = 1234567u32;
    let noise: Vec<f32> = (0..48_000)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (f64::from(state) / f64::from(u32::MAX) - 0.5) as f32
        })
        .collect();
    assert!(observe(48, &noise).frequency_hz.is_none());
    assert!(!tracks_keyboard(
        &NOTES
            .into_iter()
            .map(|note| observe(note, &noise))
            .collect::<Vec<_>>()
    ));
    assert!(!tracks_keyboard(&[]));
}

#[test]
fn inharmonic_tonal_partials_can_track_without_a_single_periodic_fundamental() {
    let observations: Vec<_> = NOTES
        .into_iter()
        .map(|note| {
            let multiplier = 2.0_f64.powf(f64::from(note - 48) / 12.0);
            let samples: Vec<f32> = (0..24000)
                .flat_map(|i| {
                    let value = [130.8, 211.2, 353.7]
                        .iter()
                        .map(|f| {
                            (std::f64::consts::TAU * f * multiplier * i as f64 / 48000.0).sin()
                        })
                        .sum::<f64>() as f32
                        * 0.1;
                    [value, value]
                })
                .collect();
            observe(note, &samples)
        })
        .collect();
    assert!(tracks_keyboard(&observations), "{observations:?}");
}
