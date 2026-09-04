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
//!
//! # スロット
//! 音源は 1 本ではなく [`SLOT_COUNT`] 本のスロットで持つ。state の綴りでスロットを選び、
//! note number でどのスロットを鳴らすかを選ぶ。綴りと対応規則は [`slots`] を参照。
//! 「小節 N を鳴らしている最中に小節 N+1 を載せておく」先読みのための仕組み。
//!
//! # 鳴っている音はスロットの差し替えで切らない
//! voice は自分が鳴らす音源の `Arc` を握るので、スロットを差し替えても最後まで鳴り切る
//! （`stop_all()` を呼ばない）。そのぶん `Arc` の解放が RT スレッドで起きうるので、
//! 要らなくなった `Arc` は [`graveyard`] へ預けて main thread に解放させる。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_extensions::{audio_ports::*, note_ports::*};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::events::UnknownEvent;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};

use crate::buffer::{CacheBuffer, VoiceBank};
use crate::graveyard::{BufferGraveyard, SharedGraveyard};
use crate::slots::{parse_state, slot_for_note, CacheSlots, StateRequest};

pub mod buffer;
pub mod graveyard;
pub mod slots;

pub use crate::slots::{slot_patch_state, SLOT_COUNT};

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
    slots: Mutex<CacheSlots>,
    generation: AtomicU64,
    /// RT スレッドが手放した `Arc` の置き場。解放は main thread が行う。
    graveyard: SharedGraveyard,
}

impl CachePlayerShared {
    /// 1 スロットぶんの音源を差し替える（main thread から呼ぶ）。
    fn set_slot(&self, slot: usize, buffer: Option<Arc<CacheBuffer>>) {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set(slot, buffer);
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// 全スロットを空にする（空 state を受けたとき）。
    fn clear_slots(&self) {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear_all();
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

        // main thread に居るいまのうちに、RT スレッドが手放した音源を解放する。
        // 先読みは小節ごとに state load を出すので、ここが定期的な引き取り口になる。
        self.shared.graveyard.reclaim();

        let mut state = String::new();
        input
            .read_to_string(&mut state)
            .map_err(|_| PluginError::Message("state を読めない"))?;
        match parse_state(&state).map_err(|_| PluginError::Message("state の綴りが不正"))? {
            StateRequest::ClearAll => self.shared.clear_slots(),
            StateRequest::Clear { slot } => self.shared.set_slot(slot, None),
            StateRequest::Load { slot, path } => {
                let buffer = CacheBuffer::load_wav(&path)
                    .map_err(|_| PluginError::Message("キャッシュ WAV を読めない"))?;
                self.shared.set_slot(slot, Some(Arc::new(buffer)));
            }
        }
        Ok(())
    }
}

pub struct CachePlayerAudioProcessor<'a> {
    shared: &'a CachePlayerShared,
    /// audio thread が持っているスロット。`generation` が動いたときだけ拾い直す。
    slots: CacheSlots,
    seen_generation: u64,
    voices: VoiceBank,
    /// 手放した `Arc` の手元の置き場。`process` の最後に共有側へ move する。
    returns: BufferGraveyard,
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
            slots: CacheSlots::default(),
            seen_generation: 0,
            voices: VoiceBank::new(),
            returns: BufferGraveyard::new(),
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        self.refresh_slots();

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
                let Some(note) = note_on_number(event) else {
                    continue;
                };
                // note number がスロットを選ぶ（[`slots::slot_for_note`]）。
                // 空のスロットを鳴らせと言われたら、前の小節の音を出さずに黙る。
                if let Some(buffer) = self.slots.get(slot_for_note(note)) {
                    let buffer = Arc::clone(buffer);
                    self.voices.note_on(buffer, &mut self.returns);
                }
            }

            // `sample_bounds()` は Range ではなく `(Bound, Bound)` なので、
            // 長さはスライスしてから取る。
            let bounds = event_batch.sample_bounds();
            let mut frames = 0usize;
            for index in 0..channel_count {
                if let Some(channel) = output_channels.channel_mut(index as u32) {
                    let slice = &mut channel[bounds];
                    frames = slice.len();
                    self.voices.mix_channel(index, slice);
                }
            }
            self.voices.advance(frames, &mut self.returns);
        }

        // 手放した `Arc` を main thread へ渡す。取れなければ手元に残して次の block で再試行。
        self.shared.graveyard.try_collect(&mut self.returns);

        if self.voices.has_active_voices() {
            Ok(ProcessStatus::Continue)
        } else {
            Ok(ProcessStatus::Sleep)
        }
    }

    /// 演奏そのものの停止。**ここでだけ `stop_all()` を呼んでよい。**
    ///
    /// 停止時なので音が切れて構わないが、解放は RT の外へ出す。手元に残ったぶんは
    /// deactivate（main thread）でこの struct ごと落ちるときに解放される。
    fn stop_processing(&mut self) {
        self.voices.stop_all(&mut self.returns);
        self.shared.graveyard.try_collect(&mut self.returns);
    }
}

/// note on として扱うイベントなら、その note number を返す。
///
/// **MIDI dialect を必ず見ること。** play server は live も offline も
/// `clap_event_midi`（生の 3 バイト）でノートを送る（`core-lib` の
/// `process_chunk_with_timing`）。CLAP note event だけを見ていると、
/// ホストからは「イベントを送ったのに無音」に見える。
///
/// CLAP note event が音高を指定していない（`Match::All`）ときは 0 を返す。
/// [`slots::slot_for_note`] が剰余を取るので、それはスロット 0 になる。
fn note_on_number(event: &UnknownEvent) -> Option<u8> {
    match event.as_core_event() {
        Some(CoreEventSpace::NoteOn(note)) => {
            Some(note.key().into_specific().unwrap_or(0).min(127) as u8)
        }
        // status の上位ニブルが 0x9、かつ velocity が 0 でないものだけ note on。
        // velocity 0 の 0x9n は note off の別表記なので数えない。
        Some(CoreEventSpace::Midi(midi)) => {
            let data = midi.data();
            (data[0] & 0xF0 == 0x90 && data[2] != 0).then_some(data[1])
        }
        _ => None,
    }
}

impl CachePlayerAudioProcessor<'_> {
    /// main thread がスロットを差し替えていたら拾い直す。**ブロックしない。**
    ///
    /// **鳴っている voice には触らない。** voice は自分が握った `Arc` を鳴らし続けるので、
    /// スロットが差し替わっても余韻が切れない（ここで `stop_all()` を呼んでいたのが
    /// 小節境界のぶつ切りの正体だった）。手放した古いスロットは graveyard へ預ける。
    fn refresh_slots(&mut self) {
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation == self.seen_generation {
            return;
        }
        let Ok(slots) = self.shared.slots.try_lock() else {
            return;
        };
        let mut previous = std::mem::replace(&mut self.slots, slots.clone());
        drop(slots);
        previous.bury_into(&mut self.returns);
        self.seen_generation = generation;
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
