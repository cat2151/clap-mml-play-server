use anyhow::Result;
use hound::{SampleFormat, WavSpec, WavWriter};
use rodio::{buffer::SamplesBuffer, ChannelCount, DeviceSinkBuilder, Player, SampleRate};

/// Vec<f32>（インターリーブステレオ）を WAVファイルに書き出す
pub fn write_wav(
    samples: &[f32],
    sample_rate: u32,
    path: impl AsRef<std::path::Path>,
) -> Result<()> {
    let path = path.as_ref();
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut wav = WavWriter::create(path, spec)
        .map_err(|e| anyhow::anyhow!("WAVファイル作成失敗 ({}): {}", path.display(), e))?;
    for &sample in samples {
        wav.write_sample(sample)
            .map_err(|e| anyhow::anyhow!("WAV書き込み失敗: {}", e))?;
    }
    wav.finalize()?;
    Ok(())
}

/// Vec<f32>（インターリーブステレオ）を 16bit PCM WAV バイト列へエンコードする。
pub fn encode_wav_i16(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    if !samples.len().is_multiple_of(2) {
        anyhow::bail!("ステレオWAVのサンプル数が奇数です");
    }

    let mut bytes = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut bytes);
        let spec = WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut wav =
            WavWriter::new(cursor, spec).map_err(|e| anyhow::anyhow!("WAV作成失敗: {}", e))?;
        for &sample in samples {
            wav.write_sample(float_sample_to_i16(sample))
                .map_err(|e| anyhow::anyhow!("WAV書き込み失敗: {}", e))?;
        }
        wav.finalize()?;
    }
    Ok(bytes)
}

fn float_sample_to_i16(sample: f32) -> i16 {
    if !sample.is_finite() {
        return 0;
    }
    if sample <= -1.0 {
        i16::MIN
    } else if sample >= 1.0 {
        i16::MAX
    } else {
        (sample * i16::MAX as f32).round() as i16
    }
}

/// 本パイプラインの出力は常にインターリーブステレオ。
const STEREO: ChannelCount = ChannelCount::new(2).unwrap();

/// Vec<f32>（インターリーブステレオ）を rodio で再生する
pub fn play_samples(samples: Vec<f32>, sample_rate: u32) -> Result<()> {
    let sample_rate =
        SampleRate::new(sample_rate).ok_or_else(|| anyhow::anyhow!("sample rate が 0 です"))?;
    // device sink を drop すると再生も止まるため、sleep_until_end まで保持する。
    let device_sink = DeviceSinkBuilder::open_default_sink()
        .map_err(|e| anyhow::anyhow!("オーディオ出力の初期化失敗: {}", e))?;
    let player = Player::connect_new(device_sink.mixer());
    let source = SamplesBuffer::new(STEREO, sample_rate, samples);
    player.append(source);
    player.sleep_until_end();
    Ok(())
}
