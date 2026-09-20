//! 鳴っている音を確実に止める。
//!
//! **CLAP の `reset()` や MIDI CC120/123 で chord を切らない。** `reset()` は voice を
//! 切る契約ではなく、Vaporizer2 は CC120/123 を繰り返すと新しい note に反応しなくなる
//! 音色がある。実際に process 済みの note を記録し、その note へ NoteOff を返す。
//!
//! クライアントが note off を送っていても届かないことがある。コマンドキューの
//! `submit_*` は軒並み `pending.clear()` するので、note off を積んだ直後に
//! `BeginLiveTimeline` が来ると、worker が拾う前に捨てられる。実測ログでも
//! `kind=midi [i0:80:60:0,...]` は受け口に届いていたのに `apply-midi` が出ていない。
//!
//! そこで「止める」の最後の砦を renderer に置く。キューの取りこぼしから独立して、
//! NoteOff は所有 bank 上の CLAP process へサンプルオフセット 0 で渡る。

use super::{LiveMidiEvent, RealtimeRenderer};
use crate::logging::emit_diagnostic;

const MIDI_NOTE_ON: u8 = 0x90;
const MIDI_NOTE_OFF: u8 = 0x80;
const MIDI_CONTROL_CHANGE: u8 = 0xB0;
const ALL_SOUND_OFF: u8 = 120;
const ALL_NOTES_OFF: u8 = 123;
const CHANNEL_COUNT: usize = 16;
const KEY_COUNT: usize = 128;

pub(super) struct ActiveNotes {
    depths: [[u16; KEY_COUNT]; CHANNEL_COUNT],
}

impl Default for ActiveNotes {
    fn default() -> Self {
        Self {
            depths: [[0; KEY_COUNT]; CHANNEL_COUNT],
        }
    }
}

impl ActiveNotes {
    pub(super) fn record(&mut self, events: &[LiveMidiEvent]) {
        for event in events {
            self.record_message(event.message);
        }
    }

    pub(super) fn record_messages(&mut self, messages: &[[u8; 3]]) {
        for message in messages {
            self.record_message(*message);
        }
    }

    fn record_message(&mut self, [status, key, velocity]: [u8; 3]) {
        let channel = usize::from(status & 0x0f);
        match status & 0xf0 {
            MIDI_NOTE_ON if velocity > 0 => {
                if let Some(depth) = self.depths[channel].get_mut(usize::from(key)) {
                    *depth = depth.saturating_add(1);
                }
            }
            MIDI_NOTE_ON | MIDI_NOTE_OFF => {
                if let Some(depth) = self.depths[channel].get_mut(usize::from(key)) {
                    *depth = depth.saturating_sub(1);
                }
            }
            MIDI_CONTROL_CHANGE if key == ALL_SOUND_OFF || key == ALL_NOTES_OFF => {
                self.depths[channel].fill(0);
            }
            _ => {}
        }
    }

    fn note_off_events(&self) -> Vec<LiveMidiEvent> {
        self.depths
            .iter()
            .enumerate()
            .flat_map(|(channel, keys)| {
                keys.iter().enumerate().flat_map(move |(key, depth)| {
                    std::iter::repeat_n(
                        LiveMidiEvent {
                            offset_frames: 0,
                            message: [MIDI_NOTE_OFF | channel as u8, key as u8, 0],
                        },
                        usize::from(*depth),
                    )
                })
            })
            .collect()
    }

    pub(super) fn clear(&mut self) {
        self.depths.fill([0; KEY_COUNT]);
    }
}

impl RealtimeRenderer {
    /// process 済みの全 note へ NoteOff を流して 1 ブロック処理する。
    ///
    /// 返ってくる音声は捨てる。イベント自体は通常の live 描画と同じ CLAP process
    /// 入力なので、plugin が広告した note dialect と単調増加する steady time を使う。
    ///
    /// **cache-player だけは NoteOff ではなく CLAP の `reset()` で切る。** あのプラグインは
    /// NoteOff を見ない（voice は WAV の最後まで鳴る契約）ので、NoteOff だけの停止では
    /// voice が render の止まった位置で残り、次の演奏の頭で続きから鳴る。
    pub fn release_all_notes(&mut self) {
        if self.processor.is_none() {
            return;
        }
        if self.keeps_voices_across_patch_load() {
            if let Some(processor) = self.processor.as_mut() {
                processor.reset();
            }
            self.active_notes.clear();
            return;
        }
        let events = self.active_notes.note_off_events();
        if events.is_empty() {
            return;
        }
        if let Err(error) = self.render_live_chunk_with_offsets(&events) {
            emit_diagnostic(format!("release all notes failed: {error:#}"));
        }
    }
}

#[cfg(test)]
mod tests;
