//! スケジュール済み再生の進行状態と、そこへ載せる曲の transport。
//!
//! イベントの発火位置（サンプル）と、プラグインへ渡す音楽的な位置（拍・小節・テンポ）を
//! 1つにまとめて持つ。テンポは「再生の属性」ではなく tempo map というデータなので、
//! 曲中で何度変わってもここを差し替えるだけで済む。

use cmrt_timeline::{TempoMapTimeline, TimelineSeconds};

use crate::midi::{SmfTempoChange, TimedMidiEvent};

#[derive(Debug, Clone)]
pub struct RealtimePlaybackSchedule {
    pub(super) events: Vec<TimedMidiEvent>,
    pub(super) total_samples: u64,
    pub(super) current_sample: u64,
    pub(super) event_cursor: usize,
    /// 曲の tempo map。プラグインへ渡す CLAP transport をここから作る。
    ///
    /// `None` だと transport が一切渡らず、tempo-sync するパッチ（delay sync /
    /// LFO sync）が自由走行になる。テンポの分かる呼び出し元は必ず載せること。
    pub(super) transport: Option<TempoMapTimeline>,
    /// 拍 0 に対応するサンプル位置。preroll を入れたぶんだけ後ろへずれる。
    ///
    /// preroll は描画後に切り落とす下駄なので、ここを 0 のままにすると小節線が
    /// preroll ぶんずれて、曲の頭が拍の途中から始まってしまう。
    musical_origin_samples: u64,
}

impl RealtimePlaybackSchedule {
    pub fn new(events: Vec<TimedMidiEvent>, total_samples: u64) -> Self {
        Self {
            events,
            total_samples,
            current_sample: 0,
            event_cursor: 0,
            transport: None,
            musical_origin_samples: 0,
        }
    }

    /// tempo map つきのスケジュール。`musical_origin_samples` は preroll のサンプル数。
    pub fn with_tempo_map(
        events: Vec<TimedMidiEvent>,
        total_samples: u64,
        tempo_map: &[SmfTempoChange],
        musical_origin_samples: u64,
    ) -> Self {
        Self {
            transport: build_transport(tempo_map),
            musical_origin_samples,
            ..Self::new(events, total_samples)
        }
    }

    /// いま描画するブロックの、拍 0 起点でのサンプル位置。
    pub(super) fn musical_sample(&self) -> u64 {
        self.current_sample
            .saturating_sub(self.musical_origin_samples)
    }

    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    pub fn current_sample(&self) -> u64 {
        self.current_sample
    }

    pub fn events(&self) -> &[TimedMidiEvent] {
        &self.events
    }

    pub fn is_finished(&self) -> bool {
        self.current_sample >= self.total_samples
    }
}

/// SMF から読んだ tempo map を transport 用のタイムラインへ組み直す。
///
/// 単調でない区間が混じっていても、その1点だけ捨てて直前のテンポで鳴らし続ける
/// （transport ごと落とすと tempo-sync が丸ごと止まるほうが痛い）。
fn build_transport(tempo_map: &[SmfTempoChange]) -> Option<TempoMapTimeline> {
    let (first, rest) = tempo_map.split_first()?;
    let mut timeline = TempoMapTimeline::new(first.tempo_bpm, first.numerator, first.denominator)?;
    for change in rest {
        let Ok(at) = TimelineSeconds::new(change.at_seconds) else {
            continue;
        };
        let _ = timeline.push(at, change.tempo_bpm, change.numerator, change.denominator);
    }
    Some(timeline)
}

#[cfg(test)]
#[path = "playback_tests.rs"]
mod tests;
