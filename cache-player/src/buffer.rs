//! キャッシュ WAV の読み込みと、再生位置を持つ voice。
//!
//! ここは CLAP に依存しない純粋なロジックだけを置く。プラグイン本体
//! （[`crate`]）はここを呼ぶだけにして、テストを CLAP 抜きで書けるようにする。

use std::sync::Arc;

use crate::graveyard::BufferGraveyard;

/// デインターリーブ済みのキャッシュ音源。
///
/// DAW の cell キャッシュ（`daw_cache/<plugin>/track{t}_meas{m}.wav`）を
/// そのまま抱える。RT スレッドでは触らず、`Arc` 越しに共有する。
pub struct CacheBuffer {
    channels: Vec<Vec<f32>>,
    sample_rate: u32,
}

impl CacheBuffer {
    /// WAV ファイルを読み込む。整数 PCM も float PCM も受け付ける。
    pub fn load_wav(path: &str) -> Result<Self, String> {
        let mut reader =
            hound::WavReader::open(path).map_err(|e| format!("WAV を開けない '{path}': {e}"))?;
        let spec = reader.spec();
        let channel_count = spec.channels.max(1) as usize;
        let mut channels = vec![Vec::new(); channel_count];

        match spec.sample_format {
            hound::SampleFormat::Float => {
                for (index, sample) in reader.samples::<f32>().enumerate() {
                    let sample = sample.map_err(|e| format!("WAV の読み出しに失敗: {e}"))?;
                    channels[index % channel_count].push(sample);
                }
            }
            hound::SampleFormat::Int => {
                // hound は bits_per_sample に関わらず i32 で読める。正規化は最大値で割る。
                let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
                for (index, sample) in reader.samples::<i32>().enumerate() {
                    let sample = sample.map_err(|e| format!("WAV の読み出しに失敗: {e}"))?;
                    channels[index % channel_count].push(sample as f32 * scale);
                }
            }
        }

        Ok(Self {
            channels,
            sample_rate: spec.sample_rate,
        })
    }

    /// 指定チャンネル。持っていないチャンネルを聞かれたら 0ch を返す
    /// （モノラルのキャッシュをステレオ出力へ流すため）。
    pub fn channel(&self, index: usize) -> &[f32] {
        self.channels
            .get(index)
            .or_else(|| self.channels.first())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// 1 チャンネルあたりのフレーム数。
    pub fn frames(&self) -> usize {
        self.channels.first().map(Vec::len).unwrap_or(0)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// テスト用。WAV を書き出さずに中身を組み立てる。
    #[cfg(test)]
    pub(crate) fn from_channels(channels: Vec<Vec<f32>>, sample_rate: u32) -> Self {
        Self {
            channels,
            sample_rate,
        }
    }
}

/// 鳴っている 1 つのキャッシュ再生。
///
/// **voice は自分が鳴らす [`CacheBuffer`] を `Arc` で握る。** スロット
/// （[`crate::slots`]）が差し替わっても、鳴っている音は最後まで鳴り切る。
/// スロット index を参照する形にすると、差し替えの瞬間に音源が入れ替わるか、
/// `stop_all()` で切るしかなくなる（それが小節境界の「ぶつ切り」の正体だった）。
pub struct Voice {
    buffer: Arc<CacheBuffer>,
    /// 先頭からのフレーム数。
    position: usize,
}

/// 同時に鳴らせる voice の数。
///
/// 小節をまたぐ余韻が重なるぶんだけあればよい。**RT スレッドで確保しない**ため
/// 固定長で持ち、溢れたら最も古い voice を潰す。
pub const MAX_VOICES: usize = 8;

/// 固定長の voice 置き場。`process` 中に一切確保しない。
///
/// **`Arc` の clone は確保ではない**（参照カウントを 1 増やすだけ）ので、
/// voice が音源を握る形にしても RT の制約は崩れない。崩れうるのは**解放**のほうなので、
/// voice を手放すメソッドはすべて [`BufferGraveyard`] を受け取り、
/// 要らなくなった `Arc` をそこへ預ける（drop しない）。
pub struct VoiceBank {
    voices: [Option<Voice>; MAX_VOICES],
    next_slot: usize,
}

impl Default for VoiceBank {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceBank {
    pub fn new() -> Self {
        Self {
            voices: std::array::from_fn(|_| None),
            next_slot: 0,
        }
    }

    /// 渡された音源を先頭から鳴らす voice を 1 つ起こす。空きが無ければ最も古いものを潰す。
    ///
    /// 潰した voice が握っていた `Arc` は `graveyard` へ預ける（RT スレッドで解放しない）。
    pub fn note_on(&mut self, buffer: Arc<CacheBuffer>, graveyard: &mut BufferGraveyard) {
        let slot = self
            .voices
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                let slot = self.next_slot;
                self.next_slot = (self.next_slot + 1) % MAX_VOICES;
                slot
            });
        let replaced = self.voices[slot].replace(Voice {
            buffer,
            position: 0,
        });
        bury_voice(replaced, graveyard);
    }

    /// 鳴っている voice をすべて止める。
    ///
    /// **`process` 中には呼ばない。** スロットの差し替えでこれを呼んでいたのが
    /// 小節境界の「ぶつ切り」の正体だった。呼んでよいのは演奏そのものの停止時
    /// （`stop_processing`）だけ。
    pub fn stop_all(&mut self, graveyard: &mut BufferGraveyard) {
        for voice in self.voices.iter_mut() {
            bury_voice(voice.take(), graveyard);
        }
    }

    pub fn has_active_voices(&self) -> bool {
        self.voices.iter().any(Option::is_some)
    }

    /// 1 チャンネルぶんを `out` へ加算する。**再生位置は進めない。**
    ///
    /// CLAP の出力バッファはチャンネルを 1 本ずつしか可変借用できないので、
    /// 「全チャンネルを混ぜてから位置を進める」の 2 段階に分けている。
    /// `out` の中身は呼び出し側でゼロ埋めしておくこと。
    ///
    /// voice ごとに音源が違うので、**別スロットの音がここで足し合わされる**。
    pub fn mix_channel(&self, channel_index: usize, out: &mut [f32]) {
        for active in self.voices.iter().flatten() {
            let source = active.buffer.channel(channel_index);
            let remaining = active.buffer.frames().saturating_sub(active.position);
            let copy_frames = remaining.min(out.len());
            for frame in 0..copy_frames {
                out[frame] += source[active.position + frame];
            }
        }
    }

    /// [`Self::mix_channel`] を全チャンネルぶん呼んだあとに、再生位置を進める。
    ///
    /// バッファ末尾に達した voice はここで終わり、握っていた `Arc` は `graveyard` へ渡る。
    pub fn advance(&mut self, frames: usize, graveyard: &mut BufferGraveyard) {
        for voice in self.voices.iter_mut() {
            let Some(active) = voice else {
                continue;
            };
            let remaining = active.buffer.frames().saturating_sub(active.position);
            active.position += remaining.min(frames);
            if active.position >= active.buffer.frames() {
                bury_voice(voice.take(), graveyard);
            }
        }
    }
}

/// 役目を終えた voice の `Arc` を graveyard へ預ける。
fn bury_voice(voice: Option<Voice>, graveyard: &mut BufferGraveyard) {
    if let Some(voice) = voice {
        graveyard.bury(voice.buffer);
    }
}

#[cfg(test)]
mod tests;
