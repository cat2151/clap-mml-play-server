//! device の callback が実際に出した波形を WAV へ録る診断用のタップ。
//!
//! `live_capture` は live の block を出力リングの手前で録るので、device 側で起きる
//! 世代の切り捨てや underrun の 0 は写らない。こちらは `fill_output` が device へ
//! 渡した (left, right) をそのまま録るので、**聴こえる波形そのもの**になる。
//!
//! 有効にするには `CMRT_OUTPUT_CAPTURE_WAV` へ出力先パスを入れる。長さは
//! `CMRT_OUTPUT_CAPTURE_SECONDS`（既定 60 秒）。最初に再生が始まった frame から
//! 録り始め、指定の長さに達したら書き出す。再生が止まっても device は 0 を出し
//! 続けるので、その間も録る。
//!
//! callback の中では確保・ロック待ち・ログをしない。バッファは有効化の時点で
//! 確保し、満ちたら容量の決まった channel で書き出し用のスレッドへ渡す。

use std::{
    path::PathBuf,
    sync::mpsc::{sync_channel, SyncSender},
};

const PATH_ENV: &str = "CMRT_OUTPUT_CAPTURE_WAV";
const SECONDS_ENV: &str = "CMRT_OUTPUT_CAPTURE_SECONDS";
const DEFAULT_CAPTURE_SECONDS: f64 = 60.0;

/// device へ出した frame を貯め、満ちたら書き出し用のスレッドへ渡す。
pub(super) struct OutputCapture {
    /// インターリーブステレオ。容量は作った時点で確保したきり伸ばさない。
    samples: Vec<f32>,
    /// 録る要素数（frame 数 × 2）。`samples` はこの長さで満ちる。
    limit: usize,
    started: bool,
    /// 満ちたバッファの渡し先。渡し終えても持ったままにする（callback の中で
    /// channel の後始末をさせないため）。
    writer: SyncSender<Vec<f32>>,
    sent: bool,
}

impl OutputCapture {
    /// `CMRT_OUTPUT_CAPTURE_WAV` が設定されているときだけ有効になる。
    pub(super) fn from_env(sample_rate: f64) -> Option<Self> {
        let path = std::env::var(PATH_ENV)
            .ok()
            .filter(|path| !path.is_empty())?;
        let seconds = std::env::var(SECONDS_ENV)
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| *s > 0.0)
            .unwrap_or(DEFAULT_CAPTURE_SECONDS);
        let sample_rate = sample_rate.round().max(1.0) as u32;
        let frames = (seconds * f64::from(sample_rate)).round().max(1.0) as usize;
        eprintln!(
            "cmrt-output-capture: event=armed path=\"{path}\" seconds={seconds} sample_rate={sample_rate}"
        );
        let path = PathBuf::from(path);
        Self::with_writer(frames, move |samples| {
            write_capture(&samples, sample_rate, &path)
        })
    }

    /// `frames` ぶんを確保し、満ちたバッファを `write` へ渡すスレッドを立てる。
    fn with_writer(frames: usize, write: impl FnOnce(Vec<f32>) + Send + 'static) -> Option<Self> {
        let (writer, receiver) = sync_channel::<Vec<f32>>(1);
        let spawned = std::thread::Builder::new()
            .name("cmrt-output-capture-writer".to_string())
            .spawn(move || {
                if let Ok(samples) = receiver.recv() {
                    write(samples);
                }
            });
        if let Err(error) = spawned {
            eprintln!("cmrt-output-capture: event=failed error=\"{error}\"");
            return None;
        }
        Some(Self {
            samples: Vec::with_capacity(frames * 2),
            limit: frames * 2,
            started: false,
            writer,
            sent: false,
        })
    }

    /// device へ出した 1 frame を録る。`playing` が初めて真になった frame から録り始める。
    #[inline]
    pub(super) fn record(&mut self, playing: bool, left: f32, right: f32) {
        if self.sent {
            return;
        }
        if !self.started {
            if !playing {
                return;
            }
            self.started = true;
        }
        self.samples.push(left);
        self.samples.push(right);
        if self.samples.len() >= self.limit {
            self.sent = true;
            // 空の Vec は確保しない。容量 1 の channel は空いているので待たない。
            let _ = self.writer.try_send(std::mem::take(&mut self.samples));
        }
    }
}

fn write_capture(samples: &[f32], sample_rate: u32, path: &std::path::Path) {
    let frames = samples.len() / 2;
    match cmrt_core::write_wav(samples, sample_rate, path) {
        Ok(()) => eprintln!(
            "cmrt-output-capture: event=written path=\"{}\" frames={frames} seconds={:.3}",
            path.display(),
            frames as f64 / f64::from(sample_rate),
        ),
        Err(error) => eprintln!(
            "cmrt-output-capture: event=failed path=\"{}\" error=\"{error:#}\"",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests;
