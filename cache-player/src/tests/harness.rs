//! テストの足場。**プラグインをホストから読み込んで実際に `process` を回す**ための
//! 最小限のホスト実装と、出力を数値で見るための WAV 生成をここへ集める。

use std::path::PathBuf;

use clack_extensions::state::PluginState;
use clack_host::events::event_types::MidiEvent;
use clack_host::events::event_types::NoteOnEvent;
use clack_host::factory::plugin::PluginFactory;
use clack_host::prelude::*;

use crate::{CachePlayerEntry, CACHE_PLAYER_PLUGIN_ID};

/// テスト用のステレオ WAV を書き出す。L/R に別々の定数を入れて、
/// チャンネルが入れ替わっていないことまで判定できるようにする。
pub(super) fn write_test_wav(name: &str, frames: usize, left: f32, right: f32) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for _ in 0..frames {
        writer.write_sample(left).unwrap();
        writer.write_sample(right).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// 位置が判る WAV を書き出す。L はフレーム番号に比例した値、R はその符号反転。
///
/// 定数を書いた WAV では「鳴っているか」しか判らない。**差し替えても再生位置が
/// 続いているか**を見るにはフレームごとに違う値が要る。1024 で割るのは f32 で
/// 誤差なく表せる値にして、期待値を厳密比較できるようにするため。
pub(super) fn write_ramp_wav(name: &str, frames: usize) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..frames {
        writer.write_sample(ramp_at(frame)).unwrap();
        writer.write_sample(-ramp_at(frame)).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// [`write_ramp_wav`] の frame 番目のサンプル値。期待値の単一ソース。
pub(super) fn ramp_at(frame: usize) -> f32 {
    frame as f32 / 1024.0
}

/// [`process_block`] が 1 回で処理するフレーム数（[`start_processing`] の設定と一致させること）。
pub(super) const BLOCK_FRAMES: usize = 32;

/// 1 ブロックの先頭で送る note on。
///
/// **2 つの dialect を両方とも通す。** play server 本体は生 MIDI
/// （`clap_event_midi`）でしか送らないが、CLAP note event で来ても鳴ることは
/// スパイクの結論として残してある。
pub(super) enum TestNotes<'a> {
    /// note を送らない。
    None,
    /// 音高を指定しない CLAP note on（`Match::All`）。スロット 0 に落ちること。
    ClapNoteOnMatchingAllKeys,
    /// 生 MIDI の note number 列。スロットを選ぶのはこの値。
    Midi(&'a [u8]),
}

/// 1 ブロック処理して出力を返す。`notes` があれば sample 0 で note on を送る。
pub(super) fn process_block(
    processor: &mut StartedPluginAudioProcessor<TestHostHandlers>,
    inputs_descriptors: &mut AudioPorts,
    outputs_descriptors: &mut AudioPorts,
    output_events: &mut EventBuffer,
    notes: TestNotes,
) -> [Vec<f32>; 2] {
    let mut input_events = EventBuffer::with_capacity(8);
    match notes {
        TestNotes::None => {}
        TestNotes::ClapNoteOnMatchingAllKeys => {
            input_events.push(&NoteOnEvent::new(0, Pckn::match_all(), 1.0));
        }
        TestNotes::Midi(notes) => {
            for note in notes {
                input_events.push(&MidiEvent::new(0, 0, [0x90, *note, 100]));
            }
        }
    }
    let mut output_buffers = [vec![0f32; 32], vec![0f32; 32]];

    let input_channels = inputs_descriptors.with_input_buffers(NO_INPUT_PORTS);
    let mut output_channels = outputs_descriptors.with_output_buffers([AudioPortBuffer {
        channels: AudioPortBufferType::f32_output_only(
            output_buffers.iter_mut().map(|b| b.as_mut_slice()),
        ),
        latency: 0,
    }]);

    processor
        .process(
            &input_channels,
            &mut output_channels,
            &input_events.as_input(),
            &mut output_events.as_output(),
            None,
            None,
        )
        .unwrap();

    output_buffers
}

/// `.clap` ファイルを置かずに entry を読む。
pub(super) fn load_entry() -> PluginEntry {
    PluginEntry::load_from_clack::<CachePlayerEntry>(c"").unwrap()
}

/// entry から instance を 1 つ作る。
pub(super) fn new_plugin(entry: &PluginEntry) -> PluginInstance<TestHostHandlers> {
    let info = HostInfo::new("cmrt-cache-player-test", "", "", "").unwrap();
    let descriptor = entry
        .get_factory::<PluginFactory>()
        .unwrap()
        .plugin_descriptor(0)
        .unwrap();
    assert_eq!(
        descriptor.id().unwrap().to_bytes(),
        CACHE_PLAYER_PLUGIN_ID.as_bytes()
    );

    PluginInstance::<TestHostHandlers>::new(
        |_| TestHostShared,
        |_| TestHostMainThread,
        entry,
        descriptor.id().unwrap(),
        &info,
    )
    .unwrap()
}

/// CLAP state を流し込む（patch 文字列と同じ綴りをそのまま渡す）。
pub(super) fn load_state(plugin: &mut PluginInstance<TestHostHandlers>, state: &str) {
    let plugin_handle = plugin.plugin_handle();
    let state_ext = plugin_handle.get_extension::<PluginState>().unwrap();
    let bytes = state.as_bytes().to_vec();
    state_ext
        .load(&plugin_handle, &mut bytes.as_slice())
        .unwrap();
}

/// CLAP state を流し込み、**成否をそのまま返す**。
///
/// [`load_state`] は `unwrap()` するので「ロードに失敗すること」を見るテストが書けない。
/// 失敗したときサーバー側は `prepare_patch` の `Err` として扱い、その instance を
/// live mix から外す（`realtime-play-server/src/player/worker/command.rs`）ので、
/// **成功したか失敗したかは音の出方を分ける分岐そのもの**。
pub(super) fn try_load_state(plugin: &mut PluginInstance<TestHostHandlers>, state: &str) -> bool {
    let plugin_handle = plugin.plugin_handle();
    let state_ext = plugin_handle.get_extension::<PluginState>().unwrap();
    let bytes = state.as_bytes().to_vec();
    state_ext
        .load(&plugin_handle, &mut bytes.as_slice())
        .is_ok()
}

/// 32 フレーム固定で activate して処理を開始する。
pub(super) fn start_processing(
    plugin: &mut PluginInstance<TestHostHandlers>,
) -> StartedPluginAudioProcessor<TestHostHandlers> {
    let configuration = PluginAudioConfiguration {
        sample_rate: 48_000.0,
        min_frames_count: 32,
        max_frames_count: 32,
    };
    plugin
        .activate(|_, _| TestHostAudioProcessor, configuration)
        .unwrap()
        .start_processing()
        .unwrap()
}

/// 入力を持たないプラグイン用の、空の入力ポートバッファ。
pub(super) type EmptyInputPort = AudioPortBuffer<
    std::iter::Empty<InputChannel<'static, f32>>,
    std::iter::Empty<InputChannel<'static, f64>>,
>;

/// 入力ポートを持たないプラグインなので、入力バッファは空で渡す。
pub(super) const NO_INPUT_PORTS: [EmptyInputPort; 0] = [];

pub(super) struct TestHostMainThread;
pub(super) struct TestHostShared;
pub(super) struct TestHostAudioProcessor;
pub(super) struct TestHostHandlers;

impl SharedHandler<'_> for TestHostShared {
    fn request_restart(&self) {
        unimplemented!()
    }

    fn request_process(&self) {
        unimplemented!()
    }

    fn request_callback(&self) {
        unimplemented!()
    }
}

impl AudioProcessorHandler<'_> for TestHostAudioProcessor {}

impl MainThreadHandler<'_> for TestHostMainThread {}

impl HostHandlers for TestHostHandlers {
    type Shared<'a> = TestHostShared;
    type MainThread<'a> = TestHostMainThread;
    type AudioProcessor<'a> = TestHostAudioProcessor;
}
