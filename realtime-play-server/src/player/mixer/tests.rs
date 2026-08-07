use super::*;

#[test]
fn sums_f32_stereo_buffers() {
    let mut mixed = vec![0.5, -0.25, 0.0, 1.0];
    add_samples(&mut mixed, &[0.25, 0.5, -0.5, 0.25], 1.0);
    assert_eq!(mixed, vec![0.75, 0.25, -0.5, 1.25]);
}

#[test]
fn applies_the_instance_gain() {
    let mut mixed = vec![0.0, 0.0, 0.0, 0.0];
    add_samples(&mut mixed, &[0.25, 0.5, -0.5, 0.25], 2.0);
    assert_eq!(mixed, vec![0.5, 1.0, -1.0, 0.5]);
}

#[test]
fn ramps_auto_gain_without_changing_stereo_balance() {
    let mut mixed = vec![0.0; 4];
    add_samples_ramped(
        &mut mixed,
        &[1.0; 4],
        GainRamp {
            start: 1.0,
            end: 2.0,
        },
    );
    assert_eq!(mixed, vec![1.5, 1.5, 2.0, 2.0]);
}

#[test]
fn auto_trim_and_auto_chord_boost_are_multiplied() {
    let auto_trim = GainRamp {
        start: 0.5,
        end: 0.5,
    };
    let mut mixed = vec![0.0; 4];

    // 完全AUTOの chord instance に付く +6 dB（振幅2倍）は、auto trim 後も維持する。
    add_samples_ramped(&mut mixed, &[1.0; 4], auto_trim.scaled(2.0));

    assert_eq!(mixed, vec![1.0; 4]);
}
