//! 鳴っている音を確実に止める。
//!
//! **CLAP の `reset()` だけでは音は止まらない。** 仕様上 `reset()` は「バッファと処理
//! 状態のリセット」であって、鳴っている voice を切ることを求めていない。実測でも
//! Surge XT は `reset()` を通しただけでは鳴り続けた。
//!
//! クライアントが note off を送っていても届かないことがある。コマンドキューの
//! `submit_*` は軒並み `pending.clear()` するので、note off を積んだ直後に
//! `BeginLiveTimeline` が来ると、worker が拾う前に捨てられる。実測ログでも
//! `kind=midi [i0:80:60:0,...]` は受け口に届いていたのに `apply-midi` が出ていない。
//!
//! そこで「止める」の最後の砦をここへ置く。キューの取りこぼしとプラグインの
//! reset 解釈の両方から独立して、音は必ず止まる。

use super::{LiveMidiEvent, RealtimeRenderer};

const MIDI_CHANNEL_COUNT: u8 = 16;
const MIDI_CONTROL_CHANGE: u8 = 0xB0;
/// CC120。release を待たずに鳴っている音を即座に切る。
const ALL_SOUND_OFF: u8 = 120;
/// CC123。押されている扱いの note を離す。CC120 と併せて出す。
const ALL_NOTES_OFF: u8 = 123;

impl RealtimeRenderer {
    /// 全 channel へ All Sound Off / All Notes Off を流して 1 ブロック処理する。
    ///
    /// CC120 は release を待たずに切る規定なので 1 ブロックで足りる。返ってくる
    /// 音声は捨てる（呼び出し元が直後に `reset()` で状態ごと捨てるため）。
    pub(super) fn silence_all_notes(&mut self) {
        if self.processor.is_none() {
            return;
        }
        let events = all_sound_off_events();
        if let Err(error) = self.render_live_chunk_with_offsets(&events) {
            eprintln!("all sound off failed: {error:#}");
        }
    }
}

fn all_sound_off_events() -> Vec<LiveMidiEvent> {
    (0..MIDI_CHANNEL_COUNT)
        .flat_map(|channel| {
            [
                [MIDI_CONTROL_CHANGE | channel, ALL_SOUND_OFF, 0],
                [MIDI_CONTROL_CHANGE | channel, ALL_NOTES_OFF, 0],
            ]
        })
        .map(|message| LiveMidiEvent {
            offset_frames: 0,
            message,
        })
        .collect()
}

#[cfg(test)]
mod tests;
