use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::Result;
use cpal::{FromSample, Sample};

pub(super) const DEFAULT_BUFFER_MULTIPLIER: u8 = 4;
const MAX_BUFFER_MULTIPLIER: usize = 8;
const PRODUCER_RETRY_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Clone, Copy)]
struct TaggedStereoFrame {
    generation: u64,
    left: f32,
    right: f32,
}

struct FrameRing {
    slots: Box<[UnsafeCell<MaybeUninit<TaggedStereoFrame>>]>,
    capacity: usize,
    write_index: AtomicUsize,
    read_index: AtomicUsize,
}

// SAFETY: FrameRing is used by exactly one producer and one consumer. The atomic indices publish
// initialized slots and prevent either side from accessing the same slot concurrently.
unsafe impl Sync for FrameRing {}

impl FrameRing {
    fn new(capacity: usize) -> Self {
        let slots = (0..capacity)
            .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            capacity,
            write_index: AtomicUsize::new(0),
            read_index: AtomicUsize::new(0),
        }
    }

    fn len(&self) -> usize {
        self.write_index
            .load(Ordering::Acquire)
            .wrapping_sub(self.read_index.load(Ordering::Acquire))
    }

    fn try_push(&self, frame: TaggedStereoFrame) -> bool {
        let write = self.write_index.load(Ordering::Relaxed);
        let read = self.read_index.load(Ordering::Acquire);
        if write.wrapping_sub(read) >= self.capacity {
            return false;
        }
        let index = write % self.capacity;
        // SAFETY: only the producer writes this unpublished slot.
        unsafe { (*self.slots[index].get()).write(frame) };
        self.write_index
            .store(write.wrapping_add(1), Ordering::Release);
        true
    }

    fn try_pop(&self) -> Option<TaggedStereoFrame> {
        let read = self.read_index.load(Ordering::Relaxed);
        let write = self.write_index.load(Ordering::Acquire);
        if read == write {
            return None;
        }
        let index = read % self.capacity;
        // SAFETY: acquire observed the producer's initialized slot and only the consumer reads it.
        let frame = unsafe { (*self.slots[index].get()).assume_init_read() };
        self.read_index
            .store(read.wrapping_add(1), Ordering::Release);
        Some(frame)
    }
}

pub(super) struct AudioOutputControl {
    generation: AtomicU64,
    active: AtomicBool,
    shutdown: AtomicBool,
    multiplier: AtomicU8,
    buffer_frames: usize,
    underrun_frames: AtomicU64,
}

impl AudioOutputControl {
    pub(super) fn start_generation(&self, generation: u64) {
        self.generation.store(generation, Ordering::Release);
        self.active.store(true, Ordering::Release);
    }

    pub(super) fn stop_generation(&self, generation: u64) {
        self.generation.store(generation, Ordering::Release);
        self.active.store(false, Ordering::Release);
    }

    pub(super) fn finish(&self) {
        self.active.store(false, Ordering::Release);
    }

    pub(super) fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.active.store(false, Ordering::Release);
    }

    pub(super) fn set_buffer_multiplier(&self, multiplier: u8) -> Result<()> {
        if !matches!(multiplier, 1 | 2 | 4 | 8) {
            anyhow::bail!("live buffer multiplier must be 1, 2, 4, or 8");
        }
        self.multiplier.store(multiplier, Ordering::Release);
        Ok(())
    }

    pub(super) fn buffer_multiplier(&self) -> u8 {
        self.multiplier.load(Ordering::Acquire)
    }

    fn target_frames(&self) -> usize {
        self.buffer_frames * self.buffer_multiplier() as usize
    }

    #[cfg(test)]
    fn underrun_frames(&self) -> u64 {
        self.underrun_frames.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

pub(super) struct AudioOutputProducer {
    ring: Arc<FrameRing>,
    control: Arc<AudioOutputControl>,
}

impl AudioOutputProducer {
    pub(super) fn wait_for_space_timeout(&self, timeout: Duration) -> bool {
        if self.control.shutdown.load(Ordering::Acquire) {
            return false;
        }
        if self.ring.len() < self.control.target_frames() {
            return true;
        }
        std::thread::park_timeout(timeout);
        !self.control.shutdown.load(Ordering::Acquire)
            && self.ring.len() < self.control.target_frames()
    }

    pub(super) fn push_chunk(&self, generation: u64, samples: Vec<f32>) -> bool {
        for frame in samples.chunks(2) {
            let tagged = TaggedStereoFrame {
                generation,
                left: frame[0],
                right: frame.get(1).copied().unwrap_or(0.0),
            };
            while !self.ring.try_push(tagged) {
                if self.control.shutdown.load(Ordering::Acquire)
                    || self.control.generation.load(Ordering::Acquire) != generation
                {
                    return false;
                }
                std::thread::park_timeout(PRODUCER_RETRY_INTERVAL);
            }
            if self.control.generation.load(Ordering::Acquire) != generation {
                return false;
            }
        }
        true
    }
}

pub(super) struct AudioOutputConsumer {
    ring: Arc<FrameRing>,
    control: Arc<AudioOutputControl>,
}

impl AudioOutputConsumer {
    pub(super) fn fill_output<T>(&mut self, output: &mut [T], channels: usize)
    where
        T: Sample + FromSample<f32>,
    {
        if channels == 0 {
            return;
        }

        for frame in output.chunks_mut(channels) {
            let (left, right) = self.next_current_frame().unwrap_or_else(|| {
                if self.control.active.load(Ordering::Acquire) {
                    self.control.underrun_frames.fetch_add(1, Ordering::Relaxed);
                }
                (0.0, 0.0)
            });
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
    }

    fn next_current_frame(&self) -> Option<(f32, f32)> {
        loop {
            let frame = self.ring.try_pop()?;
            if frame.generation == self.control.generation.load(Ordering::Acquire) {
                return Some((frame.left, frame.right));
            }
        }
    }
}

pub(super) fn new_audio_output(
    buffer_frames: usize,
) -> (
    Arc<AudioOutputControl>,
    AudioOutputProducer,
    AudioOutputConsumer,
) {
    let control = Arc::new(AudioOutputControl {
        generation: AtomicU64::new(0),
        active: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
        multiplier: AtomicU8::new(DEFAULT_BUFFER_MULTIPLIER),
        buffer_frames,
        underrun_frames: AtomicU64::new(0),
    });
    let ring = Arc::new(FrameRing::new(buffer_frames * MAX_BUFFER_MULTIPLIER));
    (
        Arc::clone(&control),
        AudioOutputProducer {
            ring: Arc::clone(&ring),
            control: Arc::clone(&control),
        },
        AudioOutputConsumer { ring, control },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_change_discards_stale_frames() {
        let (control, producer, mut consumer) = new_audio_output(2);
        control.start_generation(1);
        assert!(producer.push_chunk(1, vec![0.25, 0.75]));
        control.start_generation(2);
        assert!(producer.push_chunk(2, vec![0.5, 0.25]));
        let mut output = [1.0f32; 2];

        consumer.fill_output(&mut output, 2);

        assert_eq!(output, [0.5, 0.25]);
    }

    #[test]
    fn fill_output_mixes_down_to_mono() {
        let (control, producer, mut consumer) = new_audio_output(2);
        control.start_generation(1);
        assert!(producer.push_chunk(1, vec![0.25, 0.75]));
        let mut output = [1.0f32];

        consumer.fill_output(&mut output, 1);

        assert_eq!(output, [0.5]);
    }

    #[test]
    fn fill_output_zeroes_extra_channels() {
        let (control, producer, mut consumer) = new_audio_output(2);
        control.start_generation(1);
        assert!(producer.push_chunk(1, vec![0.25, 0.75]));
        let mut output = [1.0f32; 4];

        consumer.fill_output(&mut output, 4);

        assert_eq!(output, [0.25, 0.75, 0.0, 0.0]);
    }

    #[test]
    fn fill_output_preserves_last_odd_sample() {
        let (control, producer, mut consumer) = new_audio_output(2);
        control.start_generation(1);
        assert!(producer.push_chunk(1, vec![0.25, 0.75, 0.5]));
        let mut output = [1.0f32; 4];

        consumer.fill_output(&mut output, 2);

        assert_eq!(output, [0.25, 0.75, 0.5, 0.0]);
    }

    #[test]
    fn underrun_is_counted_only_while_active() {
        let (control, _producer, mut consumer) = new_audio_output(2);
        let mut output = [1.0f32; 2];
        consumer.fill_output(&mut output, 2);
        assert_eq!(control.underrun_frames(), 0);

        control.start_generation(1);
        consumer.fill_output(&mut output, 2);
        assert_eq!(control.underrun_frames(), 1);
    }

    #[test]
    fn multiplier_accepts_only_supported_depths() {
        let (control, _producer, _consumer) = new_audio_output(512);
        assert_eq!(control.buffer_multiplier(), 4);
        for multiplier in [1, 2, 4, 8] {
            control.set_buffer_multiplier(multiplier).unwrap();
            assert_eq!(control.buffer_multiplier(), multiplier);
        }
        assert!(control.set_buffer_multiplier(3).is_err());
    }

    #[test]
    fn multiplier_changes_render_ahead_target_without_discarding_frames() {
        let (control, producer, _consumer) = new_audio_output(2);
        control.start_generation(1);
        control.set_buffer_multiplier(2).unwrap();
        assert!(producer.push_chunk(1, vec![0.1, 0.1, 0.2, 0.2]));
        assert!(producer.wait_for_space_timeout(Duration::ZERO));
        assert!(producer.push_chunk(1, vec![0.3, 0.3, 0.4, 0.4]));
        assert!(!producer.wait_for_space_timeout(Duration::ZERO));

        control.set_buffer_multiplier(8).unwrap();
        assert!(producer.wait_for_space_timeout(Duration::ZERO));
        control.set_buffer_multiplier(1).unwrap();
        assert!(!producer.wait_for_space_timeout(Duration::ZERO));
    }
}
