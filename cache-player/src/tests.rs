use std::path::PathBuf;

use clack_extensions::state::PluginState;
use clack_host::events::event_types::NoteOnEvent;
use clack_host::factory::plugin::PluginFactory;
use clack_host::prelude::*;

use crate::buffer::CacheBuffer;
use crate::{CachePlayerEntry, CACHE_PLAYER_PLUGIN_ID};

/// テスト用のステレオ WAV を書き出す。L/R に別々の定数を入れて、
/// チャンネルが入れ替わっていないことまで判定できるようにする。
fn write_test_wav(name: &str, frames: usize, left: f32, right: f32) -> PathBuf {
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

#[test]
fn cache_buffer_reads_channels_and_sample_rate() {
    let path = write_test_wav("cmrt_cache_player_buffer.wav", 64, 0.5, -0.25);
    let buffer = CacheBuffer::load_wav(path.to_str().unwrap()).unwrap();

    assert_eq!(buffer.frames(), 64);
    // sample rate はリサンプルの要否判定に使うので、必ず読めていること。
    assert_eq!(buffer.sample_rate(), 48_000);
    assert_eq!(buffer.channel(0)[0], 0.5);
    assert_eq!(buffer.channel(1)[0], -0.25);
    // 持っていないチャンネルを聞かれたら 0ch へ落ちる（モノラル音源のステレオ出力用）。
    assert_eq!(buffer.channel(9)[0], 0.5);
}

/// スパイクの本命。`.clap` ファイルを作らずに静的リンクしたプラグインを
/// ホストから読み込み、CLAP state に WAV パスを流し、note on で鳴ることを確かめる。
#[test]
fn plays_cache_wav_loaded_via_load_from_clack() {
    let path = write_test_wav("cmrt_cache_player_play.wav", 48, 0.5, -0.25);
    let info = HostInfo::new("cmrt-cache-player-test", "", "", "").unwrap();

    // ここが検証したかった 1 点目: `.clap` の共有ライブラリを作らずに読み込める。
    let entry = PluginEntry::load_from_clack::<CachePlayerEntry>(c"").unwrap();
    let descriptor = entry
        .get_factory::<PluginFactory>()
        .unwrap()
        .plugin_descriptor(0)
        .unwrap();
    assert_eq!(
        descriptor.id().unwrap().to_bytes(),
        CACHE_PLAYER_PLUGIN_ID.as_bytes()
    );

    let mut plugin = PluginInstance::<TestHostHandlers>::new(
        |_| TestHostShared,
        |_| TestHostMainThread,
        &entry,
        descriptor.id().unwrap(),
        &info,
    )
    .unwrap();

    // 2 点目: CLAP state に WAV のパスを流すだけで音源が載る
    // （Surge の .fxp / Vaporizer2 の .vvp と同じ経路）。
    let plugin_handle = plugin.plugin_handle();
    let state_ext = plugin_handle.get_extension::<PluginState>().unwrap();
    let path_bytes = path.to_str().unwrap().as_bytes().to_vec();
    state_ext
        .load(&plugin_handle, &mut path_bytes.as_slice())
        .unwrap();

    let configuration = PluginAudioConfiguration {
        sample_rate: 48_000.0,
        min_frames_count: 32,
        max_frames_count: 32,
    };
    let processor = plugin
        .activate(|_, _| TestHostAudioProcessor, configuration)
        .unwrap();
    let mut processor = processor.start_processing().unwrap();

    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs_descriptors = AudioPorts::with_capacity(0, 0);
    let mut outputs_descriptors = AudioPorts::with_capacity(2, 1);

    // 1 ブロック目: 先頭で note on。48 フレームの音源なので 32 フレーム全部が鳴る。
    let first = process_block(
        &mut processor,
        &mut inputs_descriptors,
        &mut outputs_descriptors,
        &mut output_events,
        true,
    );
    assert_eq!(first[0][0], 0.5, "L が鳴っていない");
    assert_eq!(first[1][0], -0.25, "R が鳴っていない");
    assert_eq!(first[0][31], 0.5);

    // 2 ブロック目: 残り 16 フレームだけ鳴り、その先は無音になる。
    let second = process_block(
        &mut processor,
        &mut inputs_descriptors,
        &mut outputs_descriptors,
        &mut output_events,
        false,
    );
    assert_eq!(second[0][15], 0.5, "余りフレームが鳴っていない");
    assert_eq!(second[0][16], 0.0, "音源末尾を超えて鳴っている");

    plugin.deactivate(processor.stop_processing());
}

/// 1 ブロック処理して出力を返す。`note_on` が真なら sample 0 で note on を送る。
fn process_block(
    processor: &mut StartedPluginAudioProcessor<TestHostHandlers>,
    inputs_descriptors: &mut AudioPorts,
    outputs_descriptors: &mut AudioPorts,
    output_events: &mut EventBuffer,
    note_on: bool,
) -> [Vec<f32>; 2] {
    let mut input_events = EventBuffer::with_capacity(8);
    if note_on {
        input_events.push(&NoteOnEvent::new(0, Pckn::match_all(), 1.0));
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

/// 入力を持たないプラグイン用の、空の入力ポートバッファ。
type EmptyInputPort = AudioPortBuffer<
    std::iter::Empty<InputChannel<'static, f32>>,
    std::iter::Empty<InputChannel<'static, f64>>,
>;

/// 入力ポートを持たないプラグインなので、入力バッファは空で渡す。
const NO_INPUT_PORTS: [EmptyInputPort; 0] = [];

struct TestHostMainThread;
struct TestHostShared;
struct TestHostAudioProcessor;
struct TestHostHandlers;

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
