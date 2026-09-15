//! live mix の出力そのものを WAV へ録る診断用のタップ。
//!
//! 録る位置は limiter を通したあと・出力リングへ入れる直前。つまり
//! **ユーザーが聴くのと同じ波形**。
//!
//! なぜ要るか。「キャッシュ WAV は 1 本ずつ見ると正しいのに演奏がモタる」
//! という形の不具合は、素材ではなく**鳴らし方**の側にある。素材を 1 本ずつ
//! 調べてもそこには出ないので、混ざったあとの波形を録って測るしかない。
//!
//! 有効にするには `CMRT_LIVE_CAPTURE_WAV` へ出力先パスを入れる。長さの上限は
//! `CMRT_LIVE_CAPTURE_SECONDS`（既定 60 秒）。
//! 連続する live timeline をまたいで診断するときは
//! `CMRT_LIVE_CAPTURE_MIN_SECONDS` を指定する。その長さを超えるまで、中間の
//! StopAll で WAV を確定しない。
//!
//! **バッファは armed の時点で 1 回だけ確保する。** render スレッドで伸ばすと
//! その確保自体が音を止めるので、上限に達したらそこで録るのをやめる
//! （書き出しは停止時か worker 終了時）。

use std::path::PathBuf;

const DEFAULT_CAPTURE_SECONDS: f64 = 60.0;

/// live mix の出力を貯めておいて、演奏が止まったら WAV として書き出す。
pub(super) struct LiveCapture {
    path: PathBuf,
    sample_rate: u32,
    /// インターリーブステレオ。容量は `new` で確保したきり伸ばさない。
    samples: Vec<f32>,
    /// 最初に録ったブロックのサンプルクロック。TUI 側が予約した `at_frames` を
    /// この WAV の位置へ読み替えるための原点。
    first_clock: Option<u64>,
    /// 上限に達して録るのをやめたか。ログに出すためだけに持つ。
    truncated: bool,
    /// StopAll で書き出してよい最小フレーム数。0 なら従来どおり即時。
    minimum_frames: usize,
    /// 書き出し済み。二重に書かない。
    written: bool,
}

impl LiveCapture {
    /// `CMRT_LIVE_CAPTURE_WAV` が設定されているときだけ有効になる。
    pub(super) fn from_env(sample_rate: f64) -> Option<Self> {
        let path = std::env::var("CMRT_LIVE_CAPTURE_WAV").ok()?;
        if path.is_empty() {
            return None;
        }
        let seconds = std::env::var("CMRT_LIVE_CAPTURE_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| *s > 0.0)
            .unwrap_or(DEFAULT_CAPTURE_SECONDS);
        let sample_rate = sample_rate.round().max(1.0) as u32;
        let minimum_seconds = std::env::var("CMRT_LIVE_CAPTURE_MIN_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| *s > 0.0)
            .unwrap_or(0.0);
        let capacity = (seconds * sample_rate as f64).round() as usize * 2;
        let minimum_frames = (minimum_seconds * sample_rate as f64).round() as usize;
        eprintln!(
            "cmrt-live-capture: event=armed path=\"{path}\" seconds={seconds} \
             minimum_seconds={minimum_seconds} sample_rate={sample_rate}"
        );
        Some(Self {
            path: PathBuf::from(path),
            sample_rate,
            samples: Vec::with_capacity(capacity),
            first_clock: None,
            truncated: false,
            minimum_frames,
            written: false,
        })
    }

    /// live のブロックを 1 つ録る。上限を超えたぶんは捨てる（確保はしない）。
    pub(super) fn push(&mut self, samples: &[f32], clock_samples: u64) {
        if self.written {
            return;
        }
        if self.first_clock.is_none() {
            self.first_clock = Some(clock_samples);
        }
        let room = self.samples.capacity() - self.samples.len();
        if room == 0 {
            if !self.truncated {
                self.truncated = true;
                eprintln!(
                    "cmrt-live-capture: event=full frames={}",
                    self.samples.len() / 2
                );
            }
            return;
        }
        let take = samples.len().min(room);
        self.samples.extend_from_slice(&samples[..take]);
        if take < samples.len() && !self.truncated {
            self.truncated = true;
            eprintln!(
                "cmrt-live-capture: event=full frames={}",
                self.samples.len() / 2
            );
        }
    }

    /// StopAll 時の書き出し。指定された最小長に達するまでは録音を続ける。
    pub(super) fn finish_on_stop(&mut self) {
        let frames = self.samples.len() / 2;
        if frames < self.minimum_frames {
            eprintln!(
                "cmrt-live-capture: event=continue frames={frames} minimum_frames={}",
                self.minimum_frames
            );
            return;
        }
        self.finish();
    }

    /// 貯めたものを WAV へ書き出す。録っていなければ何もしない。
    ///
    /// 停止のたびに呼ばれるので、**2 回目以降は書かない**。1 回の演奏で 1 ファイル。
    pub(super) fn finish(&mut self) {
        if self.written || self.samples.is_empty() {
            return;
        }
        self.written = true;
        let frames = self.samples.len() / 2;
        match cmrt_core::write_wav(&self.samples, self.sample_rate, &self.path) {
            Ok(()) => eprintln!(
                "cmrt-live-capture: event=written path=\"{}\" frames={frames} seconds={:.3} first_clock={} truncated={}",
                self.path.display(),
                frames as f64 / self.sample_rate as f64,
                self.first_clock.unwrap_or(0),
                self.truncated,
            ),
            Err(error) => eprintln!(
                "cmrt-live-capture: event=failed path=\"{}\" error=\"{error:#}\"",
                self.path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests;
