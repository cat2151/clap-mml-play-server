use super::auto_gain::GainRamp;

fn add_samples(mixed: &mut [f32], samples: &[f32], gain: f32) {
    if gain == 1.0 {
        for (output, sample) in mixed.iter_mut().zip(samples) {
            *output += *sample;
        }
        return;
    }
    for (output, sample) in mixed.iter_mut().zip(samples) {
        *output += *sample * gain;
    }
}

pub(super) fn add_samples_ramped(mixed: &mut [f32], samples: &[f32], gain: GainRamp) {
    if gain.start == gain.end {
        add_samples(mixed, samples, gain.end);
        return;
    }
    let frames = samples.len().div_ceil(2).max(1);
    for (index, (output, input)) in mixed.chunks_mut(2).zip(samples.chunks(2)).enumerate() {
        let position = (index + 1) as f32 / frames as f32;
        let frame_gain = gain.start + (gain.end - gain.start) * position;
        for (output, sample) in output.iter_mut().zip(input) {
            *output += *sample * frame_gain;
        }
    }
}

#[cfg(test)]
mod tests;
