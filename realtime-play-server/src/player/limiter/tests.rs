use super::*;

#[test]
fn limits_a_linked_stereo_peak_to_the_ceiling() {
    let mut limiter = MasterLimiter::new(1_000.0);
    let mut samples = vec![0.0; 10];
    samples.extend_from_slice(&[2.0, 0.5]);
    samples.extend(vec![0.0; 12]);

    let reduction = limiter.process(&mut samples);
    let peak = samples.iter().copied().map(f32::abs).fold(0.0, f32::max);

    assert!(peak <= 10.0f32.powf(CEILING_DB / 20.0) + 1.0e-6);
    assert!(reduction.peak_db > 0.0);
    let peak_frame = samples
        .chunks_exact(2)
        .find(|frame| frame[0].abs() > 0.0)
        .unwrap();
    assert!((peak_frame[1] / peak_frame[0] - 0.25).abs() < 1.0e-6);
}

#[test]
fn introduces_exactly_five_milliseconds_of_delay() {
    let mut limiter = MasterLimiter::new(1_000.0);
    let mut samples = vec![1.0, 1.0];
    samples.extend(vec![0.0; 12]);

    limiter.process(&mut samples);

    assert!(samples[..10].iter().all(|sample| *sample == 0.0));
    assert!(samples[10] > 0.0);
}

#[test]
fn reset_clears_delay_and_gain_reduction() {
    let mut limiter = MasterLimiter::new(1_000.0);
    let mut loud = vec![2.0; 20];
    limiter.process(&mut loud);
    limiter.reset();
    let mut quiet = vec![0.25; 12];

    let reduction = limiter.process(&mut quiet);

    assert_eq!(reduction.current_db, 0.0);
    assert_eq!(quiet[10], 0.25);
}
