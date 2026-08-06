use anyhow::Result;
use cpal::{
    traits::{DeviceTrait, HostTrait},
    FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
};

use super::audio_output::AudioOutputConsumer;

pub(super) fn build_output_stream(
    audio_output: AudioOutputConsumer,
    sample_rate: f64,
) -> Result<Stream> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("既定のオーディオ出力デバイスが見つかりません"))?;
    let default_config = device
        .default_output_config()
        .map_err(|e| anyhow::anyhow!("既定の出力設定が取得できません: {e}"))?;
    let device_sample_rate = default_config.sample_rate();
    if device_sample_rate != sample_rate as u32 {
        anyhow::bail!(
            "既定の出力デバイス sample rate ({device_sample_rate}) が config.toml の sample_rate ({}) と一致しません",
            sample_rate as u32
        );
    }

    let stream_config = StreamConfig {
        channels: default_config.channels(),
        sample_rate: default_config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    match default_config.sample_format() {
        SampleFormat::F32 => {
            build_typed_output_stream::<f32>(&device, &stream_config, audio_output)
        }
        SampleFormat::I16 => {
            build_typed_output_stream::<i16>(&device, &stream_config, audio_output)
        }
        SampleFormat::U16 => {
            build_typed_output_stream::<u16>(&device, &stream_config, audio_output)
        }
        other => anyhow::bail!("未対応の出力サンプル形式です: {other:?}"),
    }
}

fn build_typed_output_stream<T>(
    device: &cpal::Device,
    stream_config: &StreamConfig,
    mut audio_output: AudioOutputConsumer,
) -> Result<Stream>
where
    T: Sample + FromSample<f32> + SizedSample,
{
    let channels = stream_config.channels as usize;
    device
        .build_output_stream(
            stream_config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                audio_output.fill_output(data, channels);
            },
            |error| eprintln!("realtime play output stream error: {error}"),
            None,
        )
        .map_err(|e| anyhow::anyhow!("オーディオ出力 stream の作成失敗: {e}"))
}
