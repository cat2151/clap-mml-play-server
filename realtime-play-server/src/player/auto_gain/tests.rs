use super::*;

const SAMPLE_RATE: f64 = 48_000.0;

fn stereo(value: f32) -> Vec<f32> {
    vec![value; 1_024]
}

#[test]
fn target_keeps_the_total_rms_headroom_constant_across_track_counts() {
    assert!((target_rms_db(2) - -12.0).abs() < 0.001);
    assert!((target_rms_db(4) - -15.0103).abs() < 0.001);
    assert!((target_rms_db(32) - -24.0412).abs() < 0.001);
}

#[test]
fn first_loud_block_is_attenuated_but_first_quiet_block_is_not_boosted() {
    let mut loud = InstanceAutoGain::default();
    let loud_ramp = loud.process_block(&stereo(1.0), SAMPLE_RATE, -18.0, true);
    assert_eq!(loud_ramp.start, 1.0);
    assert!((loud_ramp.end - amplitude_from_db(MIN_GAIN_DB)).abs() < 0.0001);

    let mut quiet = InstanceAutoGain::default();
    let quiet_ramp = quiet.process_block(&stereo(0.01), SAMPLE_RATE, -18.0, true);
    assert_eq!(quiet_ramp, GainRamp::UNITY);
}

#[test]
fn quiet_signal_rises_slowly_and_never_exceeds_the_boost_limit() {
    let mut gain = InstanceAutoGain::default();
    let samples = stereo(0.01);
    let _ = gain.process_block(&samples, SAMPLE_RATE, -18.0, true);
    let second = gain.process_block(&samples, SAMPLE_RATE, -18.0, true);
    assert!(second.end > 1.0);
    for _ in 0..2_000 {
        let _ = gain.process_block(&samples, SAMPLE_RATE, -18.0, true);
    }
    assert!((gain.gain_db - MAX_GAIN_DB).abs() < 0.001);
}

#[test]
fn silence_holds_the_learned_trim_and_disabling_resets_it() {
    let mut gain = InstanceAutoGain::default();
    let loud = stereo(1.0);
    let _ = gain.process_block(&loud, SAMPLE_RATE, -18.0, true);
    let held = gain.process_block(&stereo(0.0), SAMPLE_RATE, -18.0, true);
    assert_eq!(held.start, held.end);
    assert!(held.end < 1.0);

    assert_eq!(
        gain.process_block(&loud, SAMPLE_RATE, -18.0, false),
        GainRamp::UNITY
    );
    assert_eq!(gain.gain_db, 0.0);
    assert!(!gain.measured);
}
