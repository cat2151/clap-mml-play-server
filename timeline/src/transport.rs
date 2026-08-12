use std::fmt;

use crate::TimelineSeconds;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransportSnapshot {
    pub song_seconds: TimelineSeconds,
    pub song_beats: f64,
    pub tempo_bpm: f64,
    pub bar_start_beats: f64,
    pub bar_number: i32,
    pub time_signature_numerator: u16,
    pub time_signature_denominator: u16,
    pub playing: bool,
}

pub trait TransportTimeline {
    fn snapshot_at(&self, at: TimelineSeconds) -> Option<TransportSnapshot>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FreeRunningTimeline;

impl TransportTimeline for FreeRunningTimeline {
    fn snapshot_at(&self, _at: TimelineSeconds) -> Option<TransportSnapshot> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstantTempoTimeline {
    tempo_bpm: f64,
    numerator: u16,
    denominator: u16,
}

impl ConstantTempoTimeline {
    pub fn new(tempo_bpm: f64, numerator: u16, denominator: u16) -> Option<Self> {
        (is_valid_tempo(tempo_bpm) && is_valid_time_signature(numerator, denominator)).then_some(
            Self {
                tempo_bpm,
                numerator,
                denominator,
            },
        )
    }
}

impl TransportTimeline for ConstantTempoTimeline {
    fn snapshot_at(&self, at: TimelineSeconds) -> Option<TransportSnapshot> {
        let beats = at.get() * self.tempo_bpm / 60.0;
        Some(snapshot(
            at,
            beats,
            self.tempo_bpm,
            self.numerator,
            self.denominator,
        ))
    }
}

/// tempo map が保持できる区間の上限。
///
/// [`TempoMapTimeline::push`] はレンダースレッドから呼ばれるので、容量を先に取り切って
/// 再確保を起こさせない。上限に達したら最も古い区間を落とす。grid sequencer は grid を
/// 1周するたびに1区間しか積まないので、64 区間あれば数分ぶんの履歴になる。再生ヘッドより
/// 前の区間は [`TempoMapTimeline::snapshot_at`] の逆順探索が素通りするだけなので、
/// 落としても現在位置の答えは変わらない。
const MAX_TEMPO_SEGMENTS: usize = 64;

/// tempo map の1区間。「この絶対秒から、このテンポ・この拍子」を表す。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoSegment {
    /// この区間が始まる絶対秒（timeline 原点から）。
    pub start_seconds: TimelineSeconds,
    /// この区間が始まる拍位置。前の区間から積算する。
    pub start_beats: f64,
    pub tempo_bpm: f64,
    pub numerator: u16,
    pub denominator: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TempoMapError {
    InvalidTempo(f64),
    InvalidTimeSignature {
        numerator: u16,
        denominator: u16,
    },
    /// 末尾の区間より前へテンポ変化点を打とうとした。
    NotMonotonic {
        at: TimelineSeconds,
        last: TimelineSeconds,
    },
}

impl fmt::Display for TempoMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTempo(value) => {
                write!(f, "tempo must be finite and positive (got {value})")
            }
            Self::InvalidTimeSignature {
                numerator,
                denominator,
            } => write!(
                f,
                "time signature must be positive with a power-of-two denominator \
                 (got {numerator}/{denominator})"
            ),
            Self::NotMonotonic { at, last } => write!(
                f,
                "tempo change must not move backwards (at {} is before the last segment at {})",
                at.get(),
                last.get()
            ),
        }
    }
}

impl std::error::Error for TempoMapError {}

/// テンポ変化点の列を持つ普通の DAW 型のタイムライン。
///
/// テンポは「タイムラインの属性」ではなく「タイムライン上のデータ」なので、テンポを
/// 変えてもタイムラインの原点は動かず、作り直しも要らない。秒↔拍の変換は区間ごとに
/// 傾きが変わる区分線形になる。
///
/// 区間が1本だけのときは [`ConstantTempoTimeline`] と完全に一致する。
#[derive(Clone, Debug, PartialEq)]
pub struct TempoMapTimeline {
    segments: Vec<TempoSegment>,
}

impl TempoMapTimeline {
    /// 原点（0秒 / 0拍）から始まる1区間だけの tempo map を作る。
    pub fn new(tempo_bpm: f64, numerator: u16, denominator: u16) -> Option<Self> {
        if !is_valid_tempo(tempo_bpm) || !is_valid_time_signature(numerator, denominator) {
            return None;
        }
        let mut segments = Vec::with_capacity(MAX_TEMPO_SEGMENTS);
        segments.push(TempoSegment {
            start_seconds: TimelineSeconds::ZERO,
            start_beats: 0.0,
            tempo_bpm,
            numerator,
            denominator,
        });
        Some(Self { segments })
    }

    pub fn segments(&self) -> &[TempoSegment] {
        &self.segments
    }

    /// `at` からテンポ（と拍子）を変える。
    ///
    /// 末尾の区間より前へは打てない（打てると、既に鳴らした位置の拍が後から変わる）。
    /// 同じテンポ・同じ拍子なら区間を増やさない。同じ時刻への追記は末尾を差し替える。
    ///
    /// レンダースレッドから呼ばれるので、確保もログ出力もしないこと。
    pub fn push(
        &mut self,
        at: TimelineSeconds,
        tempo_bpm: f64,
        numerator: u16,
        denominator: u16,
    ) -> Result<(), TempoMapError> {
        if !is_valid_tempo(tempo_bpm) {
            return Err(TempoMapError::InvalidTempo(tempo_bpm));
        }
        if !is_valid_time_signature(numerator, denominator) {
            return Err(TempoMapError::InvalidTimeSignature {
                numerator,
                denominator,
            });
        }
        let last = *self.last();
        if at < last.start_seconds {
            return Err(TempoMapError::NotMonotonic {
                at,
                last: last.start_seconds,
            });
        }
        if last.tempo_bpm == tempo_bpm
            && last.numerator == numerator
            && last.denominator == denominator
        {
            return Ok(());
        }
        let segment = TempoSegment {
            start_seconds: at,
            start_beats: last.start_beats
                + (at.get() - last.start_seconds.get()) * last.tempo_bpm / 60.0,
            tempo_bpm,
            numerator,
            denominator,
        };
        if at == last.start_seconds {
            // 長さ 0 の区間を溜めても引けないので、同じ時刻なら末尾を差し替える。
            *self.segments.last_mut().expect("tempo map is never empty") = segment;
            return Ok(());
        }
        if self.segments.len() == MAX_TEMPO_SEGMENTS {
            // 先頭は再生ヘッドよりはるか後ろなので、落としても現在位置の答えは変わらない。
            self.segments.remove(0);
        }
        self.segments.push(segment);
        Ok(())
    }

    fn last(&self) -> &TempoSegment {
        self.segments.last().expect("tempo map is never empty")
    }

    fn segment_at(&self, at: TimelineSeconds) -> &TempoSegment {
        // 区間数はたかだか [`MAX_TEMPO_SEGMENTS`] で、探す位置はほぼ必ず末尾付近。
        // 二分探索より分岐が浅く、確保もしない。
        self.segments
            .iter()
            .rev()
            .find(|segment| segment.start_seconds <= at)
            // 古い区間を落としたあとに、その手前を訊かれたときだけここへ来る。
            .unwrap_or_else(|| self.segments.first().expect("tempo map is never empty"))
    }
}

impl TransportTimeline for TempoMapTimeline {
    fn snapshot_at(&self, at: TimelineSeconds) -> Option<TransportSnapshot> {
        let segment = self.segment_at(at);
        let beats = segment.start_beats
            + (at.get() - segment.start_seconds.get()) * segment.tempo_bpm / 60.0;
        Some(snapshot(
            at,
            beats,
            segment.tempo_bpm,
            segment.numerator,
            segment.denominator,
        ))
    }
}

fn snapshot(
    at: TimelineSeconds,
    beats: f64,
    tempo_bpm: f64,
    numerator: u16,
    denominator: u16,
) -> TransportSnapshot {
    let beats_per_bar = f64::from(numerator) * 4.0 / f64::from(denominator);
    let bar = (beats / beats_per_bar).floor();
    TransportSnapshot {
        song_seconds: at,
        song_beats: beats,
        tempo_bpm,
        bar_start_beats: bar * beats_per_bar,
        bar_number: bar.min(f64::from(i32::MAX)) as i32,
        time_signature_numerator: numerator,
        time_signature_denominator: denominator,
        playing: true,
    }
}

fn is_valid_tempo(tempo_bpm: f64) -> bool {
    tempo_bpm.is_finite() && tempo_bpm > 0.0
}

fn is_valid_time_signature(numerator: u16, denominator: u16) -> bool {
    numerator > 0 && denominator > 0 && denominator.is_power_of_two()
}

#[cfg(test)]
mod tests;
