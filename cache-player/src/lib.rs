//! DAW の cell キャッシュ（WAV）を鳴らすだけの CLAP プラグイン。
//!
//! # なぜプラグインなのか
//! play server の live mix は「論理 instance ごとに f32 ブロックをもらって足す」形で、
//! gain / auto gain / limiter / SHM 出力は音源の種類を問わない。その拡張点は
//! **CLAP の境界そのもの**なので、新しい音源はプラグインとして足すのが一番摩擦が少ない。
//! ネイティブな音源スロットを足すと `RealtimeRenderer`（具象 struct）の周りに union 型が
//! 要り、RT スレッドと `RendererHandoff` の unsafe 移送が同居する層を触ることになる。
//!
//! # `.clap` ファイルは作らない
//! `clack_host::entry::PluginEntry::load_from_clack` で play server のバイナリへ
//! **静的リンク**する。共有ライブラリもバンドル配置も要らず、しかも既存の
//! `PluginEntry::load`（unsafe）と違って安全な API で読み込める。
//!
//! # 音源の渡し方
//! CLAP state（`clap_plugin_state`）に **WAV のパスを UTF-8 で書く**。Surge の `.fxp` や
//! Vaporizer2 の `.vvp` を state として流しているのと同じ経路に乗るので、
//! `PreparePatch` / `PrepareStandbyPatch` をそのまま使える。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_extensions::{audio_ports::*, note_ports::*};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::events::UnknownEvent;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};

use crate::buffer::{CacheBuffer, VoiceBank};

pub mod buffer;

#[cfg(test)]
mod tests;

/// このプラグインの CLAP ID。patch routing のキーになるので変えないこと。
pub const CACHE_PLAYER_PLUGIN_ID: &str = "org.cat2151.cmrt.cache-player";

/// 静的リンク用の entry。`PluginEntry::load_from_clack::<CachePlayerEntry>` へ渡す。
pub type CachePlayerEntry = SinglePluginEntry<CachePlayerPlugin>;

pub struct CachePlayerPlugin;

impl Plugin for CachePlayerPlugin {
    type AudioProcessor<'a> = CachePlayerAudioProcessor<'a>;
    type Shared<'a> = CachePlayerShared;
    type MainThread<'a> = CachePlayerMainThread<'a>;

    fn declare_extensions(
        builder: &mut PluginExtensions<Self>,
        _shared: Option<&CachePlayerShared>,
    ) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginNotePorts>()
            .register::<PluginState>();
    }
}

impl DefaultPluginFactory for CachePlayerPlugin {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;

        PluginDescriptor::new(CACHE_PLAYER_PLUGIN_ID, "CMRT Cache Player")
            .with_features([SAMPLER, STEREO, INSTRUMENT])
    }

    fn new_shared(_host: HostSharedHandle) -> Result<CachePlayerShared, PluginError> {
        Ok(CachePlayerShared::default())
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a CachePlayerShared,
    ) -> Result<CachePlayerMainThread<'a>, PluginError> {
        Ok(CachePlayerMainThread { shared })
    }
}

/// main thread と audio thread で共有する音源。
///
/// **audio thread は `Mutex` をブロックして待たない。** `generation` が動いたときだけ
/// `try_lock` を試し、取れなければ古いバッファのまま鳴らし続ける。
#[derive(Default)]
pub struct CachePlayerShared {
    buffer: Mutex<Option<Arc<CacheBuffer>>>,
    generation: AtomicU64,
}

impl CachePlayerShared {
    /// 音源を差し替える（main thread から呼ぶ）。
    fn set_buffer(&self, buffer: Option<Arc<CacheBuffer>>) {
        *self.buffer.lock().unwrap_or_else(|e| e.into_inner()) = buffer;
        self.generation.fetch_add(1, Ordering::Release);
    }
}

impl PluginShared<'_> for CachePlayerShared {}

pub struct CachePlayerMainThread<'a> {
    shared: &'a CachePlayerShared,
}

impl<'a> PluginMainThread<'a, CachePlayerShared> for CachePlayerMainThread<'a> {}

impl PluginStateImpl for CachePlayerMainThread<'_> {
    fn save(&self, _output: &mut OutputStream) -> Result<(), PluginError> {
        // 読み込んだパスを覚えていないので空 state を返す。DAW 側は state を
        // 読み出さない（毎回パスを書き込む）ので実害はない。
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read;

        let mut path = String::new();
        input
            .read_to_string(&mut path)
            .map_err(|_| PluginError::Message("state を読めない"))?;
        let path = path.trim();
        if path.is_empty() {
            self.shared.set_buffer(None);
            return Ok(());
        }
        let buffer = CacheBuffer::load_wav(path)
            .map_err(|_| PluginError::Message("キャッシュ WAV を読めない"))?;
        self.shared.set_buffer(Some(Arc::new(buffer)));
        Ok(())
    }
}

pub struct CachePlayerAudioProcessor<'a> {
    shared: &'a CachePlayerShared,
    /// audio thread が持っている音源。`generation` が動いたときだけ拾い直す。
    buffer: Option<Arc<CacheBuffer>>,
    seen_generation: u64,
    voices: VoiceBank,
}

impl<'a> PluginAudioProcessor<'a, CachePlayerShared, CachePlayerMainThread<'a>>
    for CachePlayerAudioProcessor<'a>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &CachePlayerMainThread,
        shared: &'a CachePlayerShared,
        _audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            shared,
            buffer: None,
            seen_generation: 0,
            voices: VoiceBank::new(),
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        self.refresh_buffer();

        let mut output_port = audio
            .output_port(0)
            .ok_or(PluginError::Message("出力ポートが無い"))?;
        let mut output_channels = output_port
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("f32 の出力を期待している"))?;
        let channel_count = output_channels.channel_count() as usize;

        for index in 0..channel_count {
            if let Some(channel) = output_channels.channel_mut(index as u32) {
                channel.fill(0.0);
            }
        }

        for event_batch in events.input.batch() {
            for event in event_batch.events() {
                if is_note_on(event) {
                    // 音高は当面見ない（1 instance = 1 キャッシュ）。将来は
                    // note number を小節 index として使う。
                    self.voices.note_on();
                }
            }

            let Some(buffer) = self.buffer.as_ref() else {
                continue;
            };
            // `sample_bounds()` は Range ではなく `(Bound, Bound)` なので、
            // 長さはスライスしてから取る。
            let bounds = event_batch.sample_bounds();
            let mut frames = 0usize;
            for index in 0..channel_count {
                if let Some(channel) = output_channels.channel_mut(index as u32) {
                    let slice = &mut channel[bounds];
                    frames = slice.len();
                    self.voices.mix_channel(buffer, index, slice);
                }
            }
            self.voices.advance(buffer, frames);
        }

        if self.voices.has_active_voices() {
            Ok(ProcessStatus::Continue)
        } else {
            Ok(ProcessStatus::Sleep)
        }
    }

    fn stop_processing(&mut self) {
        self.voices.stop_all();
    }
}

/// note on として扱うイベントか。
///
/// **MIDI dialect を必ず見ること。** play server は live も offline も
/// `clap_event_midi`（生の 3 バイト）でノートを送る（`core-lib` の
/// `process_chunk_with_timing`）。CLAP note event だけを見ていると、
/// ホストからは「イベントを送ったのに無音」に見える。
fn is_note_on(event: &UnknownEvent) -> bool {
    match event.as_core_event() {
        Some(CoreEventSpace::NoteOn(_)) => true,
        // status の上位ニブルが 0x9、かつ velocity が 0 でないものだけ note on。
        // velocity 0 の 0x9n は note off の別表記なので数えない。
        Some(CoreEventSpace::Midi(midi)) => {
            let data = midi.data();
            data[0] & 0xF0 == 0x90 && data[2] != 0
        }
        _ => false,
    }
}

impl CachePlayerAudioProcessor<'_> {
    /// main thread が音源を差し替えていたら拾い直す。**ブロックしない。**
    fn refresh_buffer(&mut self) {
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation == self.seen_generation {
            return;
        }
        let Ok(buffer) = self.shared.buffer.try_lock() else {
            return;
        };
        self.buffer = buffer.clone();
        self.seen_generation = generation;
        self.voices.stop_all();
    }
}

impl PluginAudioPortsImpl for CachePlayerMainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        if is_input {
            0
        } else {
            1
        }
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if !is_input && index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(1),
                name: b"main",
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            });
        }
    }
}

impl PluginNotePortsImpl for CachePlayerMainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        if is_input {
            1
        } else {
            0
        }
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if is_input && index == 0 {
            writer.set(&NotePortInfo {
                id: ClapId::new(1),
                name: b"main",
                // play server は生 MIDI で送ってくるので MIDI を第一にする。
                preferred_dialect: Some(NoteDialect::Midi),
                supported_dialects: NoteDialects::CLAP | NoteDialects::MIDI,
            })
        }
    }
}
