//! サンプルオフセット付き live MIDI を CLAP process へ渡す。

use anyhow::Result;
use clack_host::events::event_types::{MidiEvent as ClapMidiEvent, NoteOffEvent, NoteOnEvent};
use clack_host::events::EventFlags;
use clack_host::events::Match;
use clack_host::prelude::*;
use cmrt_clack_timeline::{process_block_timing, ProcessBlockTiming};
use cmrt_timeline::{BlockSpan, FreeRunningTimeline, SamplePosition};

use super::{NoteEventDialect, RealtimeRenderer};

/// live MIDI 1.0 short message と、その chunk 先頭からのフレームオフセット。
///
/// オフセットは呼び出し側が chunk 境界へ割り付け済みであること。
/// [`RealtimeRenderer::render_live_chunk_with_offsets`] は `buf_size - 1` を超える
/// オフセットをクランプするだけで、次 chunk へ持ち越さない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveMidiEvent {
    pub offset_frames: u32,
    pub message: [u8; 3],
}

impl RealtimeRenderer {
    /// timestampを持たないlive MIDI 1.0 short message群を、順序を保って
    /// 次のchunk先頭で処理する。複数のNote Onは同時発音としてpluginへ渡る。
    pub fn render_live_chunk(&mut self, midi_messages: &[[u8; 3]]) -> Result<Vec<f32>> {
        let events = midi_messages
            .iter()
            .map(|message| LiveMidiEvent {
                offset_frames: 0,
                message: *message,
            })
            .collect::<Vec<_>>();
        self.render_live_chunk_with_offsets(&events)
    }

    /// chunk 内オフセット付きの live 描画。オフセットはサンプル精度でpluginへ渡る。
    ///
    /// イベントは `offset_frames` 昇順で渡すこと（CLAP のイベントリストは時刻順が前提）。
    pub fn render_live_chunk_with_offsets(&mut self, events: &[LiveMidiEvent]) -> Result<Vec<f32>> {
        let block = BlockSpan::new(
            SamplePosition(self.process_cursor_samples),
            self.buf_size as u32,
        )
        .map_err(|error| anyhow::anyhow!(error))?;
        let timing = process_block_timing(block, self.sample_rate, &FreeRunningTimeline);
        self.render_live_chunk_with_timing(events, timing)
    }

    /// 明示的なtransportとともにlive blockを描く。CLAP `steady_time` はrenderer内で
    /// 単調増加し、音楽timelineだけを張り直しても後戻りしない。
    pub fn render_live_chunk_with_timing(
        &mut self,
        events: &[LiveMidiEvent],
        mut timing: ProcessBlockTiming,
    ) -> Result<Vec<f32>> {
        let events = self.take_pending_release(events);
        let events = events.as_ref();
        let last_frame = self.buf_size.saturating_sub(1) as u32;
        let mut input_events_raw = EventBuffer::new();
        for event in events {
            let offset = event.offset_frames.min(last_frame);
            push_live_event(
                &mut input_events_raw,
                offset,
                event.message,
                self.note_dialect,
            );
        }
        timing.steady_time = self.process_cursor_samples;
        let processed =
            self.process_chunk_with_timing(self.buf_size as u32, &input_events_raw, timing)?;
        self.active_notes.record(events);
        Ok(processed.samples)
    }
}

fn push_live_event(
    events: &mut EventBuffer,
    offset: u32,
    message: [u8; 3],
    dialect: NoteEventDialect,
) {
    let [status, key, velocity] = message;
    let channel = u16::from(status & 0x0f);
    let key = u16::from(key & 0x7f);
    match (dialect, status & 0xf0, velocity) {
        (NoteEventDialect::Clap, 0x90, 1..=u8::MAX) => events.push(
            &NoteOnEvent::new(
                offset,
                Pckn::new(0u16, channel, key, Match::All),
                f64::from(velocity & 0x7f) / 127.0,
            )
            .with_flags(EventFlags::IS_LIVE),
        ),
        (NoteEventDialect::Clap, 0x80 | 0x90, _) => events.push(
            &NoteOffEvent::new(
                offset,
                Pckn::new(0u16, channel, key, Match::All),
                f64::from(velocity & 0x7f) / 127.0,
            )
            .with_flags(EventFlags::IS_LIVE),
        ),
        _ => events.push(&ClapMidiEvent::new(offset, 0, message).with_flags(EventFlags::IS_LIVE)),
    }
}
