//! effect 1 個ぶんの CLAP instance と、その 1 block 処理。

use std::fmt;

use anyhow::{bail, Result};
use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::latency::PluginLatency;
use clack_extensions::params::{ParamInfoBuffer, PluginParams};
use clack_host::events::event_types::TransportEvent;
use clack_host::prelude::*;
use cmrt_timeline::SampleRate;

use crate::host::MidiRenderHost;
use crate::render::{
    create_plugin_instance_without_patch, load_plugin_state, save_plugin_state, select_descriptor,
};

/// [`EffectRenderer::process`] の失敗。RT で使うので文字列を持たない。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectProcessError {
    /// [`EffectRenderer::ensure_active`] の前に呼ばれた。
    NotActive,
    /// `samples` の長さが `2 * buf_size` でない。
    BlockLength { expected: usize, actual: usize },
    /// plugin の `process()` が失敗を返した。
    Plugin,
}

impl fmt::Display for EffectProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotActive => write!(f, "effect が activate されていない"),
            Self::BlockLength { expected, actual } => {
                write!(f, "effect の block 長が {expected} でなく {actual}")
            }
            Self::Plugin => write!(f, "effect の process() が失敗"),
        }
    }
}

impl std::error::Error for EffectProcessError {}

pub struct EffectRenderer {
    instance: PluginInstance<MidiRenderHost>,
    processor: Option<StartedPluginAudioProcessor<MidiRenderHost>>,
    plugin_id: String,
    /// preset を載せる前の state。preset → state の組み立ての template になる。
    init_state: Vec<u8>,
    /// input port ごとの channel buffer。
    inputs: Vec<Vec<Vec<f32>>>,
    out_left: Vec<f32>,
    out_right: Vec<f32>,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    output_events: EventBuffer,
    /// CLAP `steady_time`。activate からの累計で、後戻りしない。
    cursor: u64,
    sample_rate: SampleRate,
    buf_size: usize,
}

impl EffectRenderer {
    /// instance を作って init state を取る。activate はまだしない
    /// （preset は activate 前に載せるほうが速い。TONE3000 は activate 中の load が 100 倍遅い）。
    pub fn new(
        entry: &PluginEntry,
        plugin_id: &str,
        sample_rate: f64,
        buf_size: usize,
    ) -> Result<Self> {
        if buf_size == 0 {
            bail!("effect の buf_size が 0");
        }
        let descriptor = select_descriptor(entry, Some(plugin_id))?;
        let mut instance = create_plugin_instance_without_patch(entry, &descriptor)?;
        let (input_channels, main_output) = audio_port_layout(&mut instance)?;
        if main_output != 2 {
            bail!(
                "effect '{}' の main output が {main_output} ch（2 ch が要る）",
                descriptor.id
            );
        }
        let init_state = save_plugin_state(&mut instance)?;
        let inputs: Vec<Vec<Vec<f32>>> = input_channels
            .iter()
            .map(|channels| vec![vec![0.0; buf_size]; *channels as usize])
            .collect();
        let total_input_channels: usize = input_channels.iter().sum::<u32>() as usize;
        Ok(Self {
            instance,
            processor: None,
            plugin_id: descriptor.id,
            init_state,
            inputs,
            out_left: vec![0.0; buf_size],
            out_right: vec![0.0; buf_size],
            input_ports: AudioPorts::with_capacity(total_input_channels, input_channels.len()),
            output_ports: AudioPorts::with_capacity(2, 1),
            output_events: EventBuffer::new(),
            cursor: 0,
            sample_rate: SampleRate::new(sample_rate).map_err(|error| anyhow::anyhow!(error))?,
            buf_size,
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn buf_size(&self) -> usize {
        self.buf_size
    }

    pub fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    pub fn init_state(&self) -> &[u8] {
        &self.init_state
    }

    /// input port ごとの channel 数（広告された順）。
    pub fn input_port_channels(&self) -> Vec<usize> {
        self.inputs.iter().map(Vec::len).collect()
    }

    pub fn is_active(&self) -> bool {
        self.processor.is_some()
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>> {
        save_plugin_state(&mut self.instance)
    }

    /// state を読ませる。処理中なら止めてから読ませ、読ませたあと再開する。
    pub fn load_state(&mut self, state: &[u8]) -> Result<()> {
        let stopped = self
            .processor
            .take()
            .map(|processor| processor.stop_processing());
        let result = load_plugin_state(&mut self.instance, state);
        if let Some(stopped) = stopped {
            self.processor = Some(
                stopped
                    .start_processing()
                    .map_err(|error| anyhow::anyhow!("start_processing 失敗: {error:?}"))?,
            );
        }
        result
    }

    /// activate + start_processing。済んでいれば何もしない。
    pub fn ensure_active(&mut self) -> Result<()> {
        if self.processor.is_some() {
            return Ok(());
        }
        let config = PluginAudioConfiguration {
            sample_rate: self.sample_rate.get(),
            min_frames_count: self.buf_size as u32,
            max_frames_count: self.buf_size as u32,
        };
        let processor = self.instance.activate(|_, _| (), config)?;
        self.processor = Some(
            processor
                .start_processing()
                .map_err(|error| anyhow::anyhow!("start_processing 失敗: {error:?}"))?,
        );
        Ok(())
    }

    /// `clap.latency` の値（frame）。拡張が無ければ 0。
    pub fn latency(&mut self) -> u32 {
        let handle = self.instance.plugin_handle();
        handle
            .get_extension::<PluginLatency>()
            .map_or(0, |latency| latency.get(&handle))
    }

    /// `clap.params` が名乗る parameter 名を index 順に返す。
    pub fn param_names(&mut self) -> Vec<String> {
        let handle = self.instance.plugin_handle();
        let Some(params) = handle.get_extension::<PluginParams>() else {
            return Vec::new();
        };
        (0..params.count(&handle))
            .filter_map(|index| {
                let mut buffer = ParamInfoBuffer::new();
                params
                    .get_info(&handle, index, &mut buffer)
                    .map(|info| String::from_utf8_lossy(info.name).into_owned())
            })
            .collect()
    }

    /// plugin が要求した main-thread callback を 1 回ぶん処理する。処理したら `true`。
    pub(super) fn pump_main_thread(&mut self) -> bool {
        let requested = self
            .instance
            .access_shared_handler(|host| host.take_callback_request());
        if requested {
            self.instance.call_on_main_thread_callback();
        }
        requested
    }

    /// 内部の遅延線・尻尾を捨てる。activate 前なら何もしない。
    pub fn reset(&mut self) {
        if let Some(processor) = self.processor.as_mut() {
            processor.reset();
        }
    }

    /// interleaved stereo の 1 block（`2 * buf_size` 個）を in-place で通す。
    ///
    /// `transport` は instrument に渡したものと同じ block のもの。tempo-sync する
    /// delay はこれが無いと自由走行になる。
    pub fn process(
        &mut self,
        samples: &mut [f32],
        transport: Option<&TransportEvent>,
    ) -> Result<(), EffectProcessError> {
        let frames = self.buf_size;
        if samples.len() != 2 * frames {
            return Err(EffectProcessError::BlockLength {
                expected: 2 * frames,
                actual: samples.len(),
            });
        }
        let processor = self
            .processor
            .as_mut()
            .ok_or(EffectProcessError::NotActive)?;
        for port in &mut self.inputs {
            for channel in port.iter_mut() {
                channel.fill(0.0);
            }
        }
        if let Some(port) = self.inputs.first_mut() {
            for (index, frame) in samples.as_chunks::<2>().0.iter().enumerate() {
                for (channel, sample) in port.iter_mut().zip(frame) {
                    channel[index] = *sample;
                }
            }
        }
        self.out_left.fill(0.0);
        self.out_right.fill(0.0);
        self.output_events.clear();

        let input_events_raw = EventBuffer::new();
        let input_events = InputEvents::from_buffer(&input_events_raw);
        let input_buffers = self.inputs.iter_mut().map(|port| AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                port.iter_mut()
                    .map(|channel| InputChannel::constant(channel.as_mut_slice())),
            ),
        });
        let input_audio = self.input_ports.with_input_buffers(input_buffers);
        let out_l: &mut [f32] = &mut self.out_left;
        let out_r: &mut [f32] = &mut self.out_right;
        let mut output_audio = self.output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only([out_l, out_r].into_iter()),
        }]);
        {
            let mut output_events = OutputEvents::from_buffer(&mut self.output_events);
            processor
                .process(
                    &input_audio,
                    &mut output_audio,
                    &input_events,
                    &mut output_events,
                    Some(self.cursor),
                    transport,
                )
                .map_err(|_| EffectProcessError::Plugin)?;
        }
        self.cursor = self.cursor.saturating_add(frames as u64);
        for (index, frame) in samples.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            frame[0] = self.out_left[index];
            frame[1] = self.out_right[index];
        }
        Ok(())
    }
}

impl Drop for EffectRenderer {
    fn drop(&mut self) {
        if let Some(processor) = self.processor.take() {
            let stopped = processor.stop_processing();
            self.instance.deactivate(stopped);
        }
    }
}

/// (input port ごとの channel 数, main output の channel 数)。output port が無ければエラー。
fn audio_port_layout(instance: &mut PluginInstance<MidiRenderHost>) -> Result<(Vec<u32>, u32)> {
    let handle = instance.plugin_handle();
    let audio_ports = handle
        .get_extension::<PluginAudioPorts>()
        .ok_or_else(|| anyhow::anyhow!("effect に audio-ports 拡張が無い"))?;
    let mut buffer = AudioPortInfoBuffer::new();
    let input_channels: Vec<u32> = (0..audio_ports.count(&handle, true))
        .map(|port| {
            audio_ports
                .get(&handle, port, true, &mut buffer)
                .map_or(0, |info| info.channel_count)
        })
        .collect();
    if audio_ports.count(&handle, false) == 0 {
        bail!("effect に audio output port が無い");
    }
    let main_output = audio_ports
        .get(&handle, 0, false, &mut buffer)
        .map_or(0, |info| info.channel_count);
    Ok((input_channels, main_output))
}
