//! Audio block processing and output event collection.
use super::{ProcessedChunk, RealtimeRenderer};
use anyhow::Result;
use clack_host::events::spaces::CoreEventSpace;
use clack_host::prelude::*;
use cmrt_clack_timeline::{process_block_timing, ProcessBlockTiming};
use cmrt_timeline::{BlockSpan, FreeRunningTimeline, SamplePosition};
impl RealtimeRenderer {
    pub(super) fn process_chunk_with_events(
        &mut self,
        frames: u32,
        input_events_raw: &EventBuffer,
    ) -> Result<ProcessedChunk> {
        let block = BlockSpan::new(SamplePosition(self.process_cursor_samples), frames)
            .map_err(|error| anyhow::anyhow!(error))?;
        let timing = process_block_timing(block, self.sample_rate, &FreeRunningTimeline);
        self.process_chunk_with_timing(frames, input_events_raw, timing)
    }

    pub(super) fn process_chunk_with_timing(
        &mut self,
        frames: u32,
        input_events_raw: &EventBuffer,
        timing: ProcessBlockTiming,
    ) -> Result<ProcessedChunk> {
        let input_events = InputEvents::from_buffer(input_events_raw);
        self.output_events_buf.clear();
        let frame_len = frames as usize;
        self.out_left[..frame_len].fill(0.0);
        self.out_right[..frame_len].fill(0.0);
        let out_l: &mut [f32] = &mut self.out_left[..frame_len];
        let out_r: &mut [f32] = &mut self.out_right[..frame_len];
        // input port を持たないプラグインへは buffer を 1 本も渡さない。clack は port が
        // 0 件なら input 側の frames を数えず、output 側の frames をブロック長に採用する。
        let mut input_buffers = Vec::with_capacity(self.capabilities.audio_input_ports as usize);
        if self.capabilities.audio_input_ports > 0 {
            let in_l: &mut [f32] = &mut self.in_left[..frame_len];
            let in_r: &mut [f32] = &mut self.in_right[..frame_len];
            input_buffers.push(AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only(
                    [in_l, in_r].into_iter().map(InputChannel::constant),
                ),
            });
        }
        let input_audio = self.input_ports.with_input_buffers(input_buffers);
        let mut output_audio = self.output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only([out_l, out_r].into_iter()),
        }]);
        {
            let mut output_events = OutputEvents::from_buffer(&mut self.output_events_buf);
            self.processor
                .as_mut()
                .expect("processor is always present while renderer is alive")
                .process(
                    &input_audio,
                    &mut output_audio,
                    &input_events,
                    &mut output_events,
                    Some(timing.steady_time),
                    timing.transport.as_ref(),
                )
                .map_err(|e| anyhow::anyhow!("process() 失敗: {:?}", e))?;
        }
        self.process_cursor_samples = self
            .process_cursor_samples
            .saturating_add(u64::from(frames));

        let ended_note_ids = self
            .output_events_buf
            .iter()
            .filter_map(|event| match event.as_core_event() {
                Some(CoreEventSpace::NoteEnd(note_end)) => note_end.pckn().note_id.into_specific(),
                _ => None,
            })
            .collect();

        let mut samples = Vec::with_capacity(frame_len * 2);
        for i in 0..frame_len {
            samples.push(self.out_left[i]);
            samples.push(self.out_right[i]);
        }
        Ok(ProcessedChunk {
            samples,
            ended_note_ids,
        })
    }
}
