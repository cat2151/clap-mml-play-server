use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex},
    time::Duration,
};

use cpal::{FromSample, Sample};

const MAX_BUFFERED_CHUNKS: usize = 8;

#[derive(Default)]
pub(super) struct AudioOutputBuffer {
    state: Mutex<AudioOutputState>,
    state_changed: Condvar,
}

#[derive(Default)]
struct AudioOutputState {
    generation: u64,
    chunks: VecDeque<AudioChunk>,
    shutdown: bool,
}

struct AudioChunk {
    samples: Vec<f32>,
    offset: usize,
}

impl AudioOutputBuffer {
    pub(super) fn reset(&self, generation: u64) {
        let mut state = self.state.lock().unwrap();
        state.generation = generation;
        state.chunks.clear();
        self.state_changed.notify_all();
    }

    pub(super) fn shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        state.shutdown = true;
        state.chunks.clear();
        self.state_changed.notify_all();
    }

    pub(super) fn wait_for_space_timeout(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.shutdown || state.chunks.len() < MAX_BUFFERED_CHUNKS {
            return !state.shutdown;
        }
        let (state_after_wait, _) = self.state_changed.wait_timeout(state, timeout).unwrap();
        state = state_after_wait;
        !state.shutdown && state.chunks.len() < MAX_BUFFERED_CHUNKS
    }

    pub(super) fn push_chunk(&self, generation: u64, samples: Vec<f32>) -> bool {
        let mut state = self.state.lock().unwrap();
        while !state.shutdown
            && state.generation == generation
            && state.chunks.len() >= MAX_BUFFERED_CHUNKS
        {
            state = self.state_changed.wait(state).unwrap();
        }
        if state.shutdown || state.generation != generation {
            return false;
        }
        state.chunks.push_back(AudioChunk { samples, offset: 0 });
        true
    }

    pub(super) fn fill_output<T>(&self, output: &mut [T], channels: usize)
    where
        T: Sample + FromSample<f32>,
    {
        if channels == 0 {
            return;
        }

        let Ok(mut state) = self.state.try_lock() else {
            zero_output(output);
            return;
        };
        let mut consumed = false;
        for frame in output.chunks_mut(channels) {
            let (left, right) = next_stereo_sample(&mut state).unwrap_or((0.0, 0.0));
            consumed = true;
            if channels == 1 {
                frame[0] = T::from_sample((left + right) * 0.5);
                continue;
            }
            frame[0] = T::from_sample(left);
            frame[1] = T::from_sample(right);
            for sample in &mut frame[2..] {
                *sample = T::from_sample(0.0);
            }
        }
        if consumed {
            self.state_changed.notify_all();
        }
    }
}

fn next_stereo_sample(state: &mut AudioOutputState) -> Option<(f32, f32)> {
    loop {
        let chunk = state.chunks.front_mut()?;
        if chunk.offset >= chunk.samples.len() {
            state.chunks.pop_front();
            continue;
        }
        let left = chunk.samples[chunk.offset];
        let right = chunk.samples.get(chunk.offset + 1).copied().unwrap_or(0.0);
        chunk.offset += 2;
        if chunk.offset >= chunk.samples.len() {
            state.chunks.pop_front();
        }
        return Some((left, right));
    }
}

fn zero_output<T>(output: &mut [T])
where
    T: Sample + FromSample<f32>,
{
    for sample in output {
        *sample = T::from_sample(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_discards_stale_generation_chunks() {
        let buffer = AudioOutputBuffer::default();
        buffer.reset(1);
        assert!(buffer.push_chunk(1, vec![0.25, 0.75]));

        buffer.reset(2);

        assert!(!buffer.push_chunk(1, vec![0.5, 0.5]));
    }

    #[test]
    fn fill_output_mixes_down_to_mono() {
        let buffer = AudioOutputBuffer::default();
        buffer.reset(1);
        assert!(buffer.push_chunk(1, vec![0.25, 0.75]));
        let mut output = [1.0f32];

        buffer.fill_output(&mut output, 1);

        assert_eq!(output, [0.5]);
    }

    #[test]
    fn fill_output_zeroes_extra_channels() {
        let buffer = AudioOutputBuffer::default();
        buffer.reset(1);
        assert!(buffer.push_chunk(1, vec![0.25, 0.75]));
        let mut output = [1.0f32; 4];

        buffer.fill_output(&mut output, 4);

        assert_eq!(output, [0.25, 0.75, 0.0, 0.0]);
    }

    #[test]
    fn fill_output_preserves_last_odd_sample() {
        let buffer = AudioOutputBuffer::default();
        buffer.reset(1);
        assert!(buffer.push_chunk(1, vec![0.25, 0.75, 0.5]));
        let mut output = [1.0f32; 4];

        buffer.fill_output(&mut output, 2);

        assert_eq!(output, [0.25, 0.75, 0.5, 0.0]);
    }
}
