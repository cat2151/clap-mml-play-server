use std::collections::VecDeque;

use crate::{BlockSpan, SamplePosition, SampleRate, TimelineError, TimelineSeconds};

#[derive(Clone, Debug, PartialEq)]
pub struct Timed<T> {
    pub at: TimelineSeconds,
    pub payload: T,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuantizedTime {
    pub requested: TimelineSeconds,
    pub sample: SamplePosition,
    pub error_seconds: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LateEventPolicy {
    ClampToBlockStart,
    Drop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockEvent<T> {
    pub requested: TimelineSeconds,
    pub quantized_sample: SamplePosition,
    pub offset_frames: u32,
    pub late_by_samples: u64,
    pub payload: T,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockEvents<T> {
    pub events: Vec<BlockEvent<T>>,
    pub dropped_late_events: u64,
    pub max_late_by_samples: u64,
}

struct Queued<T> {
    sequence: u64,
    time: QuantizedTime,
    payload: T,
}

pub struct BlockScheduler<T> {
    sample_rate: SampleRate,
    next_sequence: u64,
    pending: VecDeque<Queued<T>>,
}

impl<T> BlockScheduler<T> {
    pub fn new(sample_rate: SampleRate) -> Self {
        Self {
            sample_rate,
            next_sequence: 0,
            pending: VecDeque::new(),
        }
    }

    pub fn schedule(&mut self, event: Timed<T>) -> Result<QuantizedTime, TimelineError> {
        let sample = self.sample_rate.seconds_to_sample(event.at)?;
        let quantized = self.sample_rate.sample_to_seconds(sample);
        let time = QuantizedTime {
            requested: event.at,
            sample,
            error_seconds: quantized.get() - event.at.get(),
        };
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let insert_at = self
            .pending
            .partition_point(|queued| (queued.time.sample, queued.sequence) <= (sample, sequence));
        self.pending.insert(
            insert_at,
            Queued {
                sequence,
                time,
                payload: event.payload,
            },
        );
        Ok(time)
    }

    pub fn take_block(&mut self, block: BlockSpan, late_policy: LateEventPolicy) -> BlockEvents<T> {
        let take = self
            .pending
            .partition_point(|queued| queued.time.sample < block.end());
        let mut events = Vec::with_capacity(take);
        let mut dropped_late_events = 0u64;
        let mut max_late_by_samples = 0u64;
        for queued in self.pending.drain(..take) {
            let late_by_samples = block.start().0.saturating_sub(queued.time.sample.0);
            max_late_by_samples = max_late_by_samples.max(late_by_samples);
            if late_by_samples > 0 && late_policy == LateEventPolicy::Drop {
                dropped_late_events = dropped_late_events.saturating_add(1);
                continue;
            }
            let offset = queued
                .time
                .sample
                .0
                .saturating_sub(block.start().0)
                .min(u64::from(block.frames() - 1)) as u32;
            events.push(BlockEvent {
                requested: queued.time.requested,
                quantized_sample: queued.time.sample,
                offset_frames: offset,
                late_by_samples,
                payload: queued.payload,
            });
        }
        BlockEvents {
            events,
            dropped_late_events,
            max_late_by_samples,
        }
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
