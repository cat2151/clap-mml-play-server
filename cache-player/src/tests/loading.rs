//! state で音源を載せて鳴らす、いちばん基本の経路。

use clack_host::prelude::*;

use crate::buffer::CacheBuffer;
use crate::tests::harness::*;

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

    // ここが検証したかった 1 点目: `.clap` の共有ライブラリを作らずに読み込める。
    let entry = load_entry();
    let mut plugin = new_plugin(&entry);

    // 2 点目: CLAP state に WAV のパスを流すだけで音源が載る
    // （Surge の .fxp / Vaporizer2 の .vvp と同じ経路）。
    load_state(&mut plugin, path.to_str().unwrap());

    let mut processor = start_processing(&mut plugin);

    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs_descriptors = AudioPorts::with_capacity(0, 0);
    let mut outputs_descriptors = AudioPorts::with_capacity(2, 1);

    // 1 ブロック目: 先頭で note on。48 フレームの音源なので 32 フレーム全部が鳴る。
    let first = process_block(
        &mut processor,
        &mut inputs_descriptors,
        &mut outputs_descriptors,
        &mut output_events,
        TestNotes::ClapNoteOnMatchingAllKeys,
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
        TestNotes::None,
    );
    assert_eq!(second[0][15], 0.5, "余りフレームが鳴っていない");
    assert_eq!(second[0][16], 0.0, "音源末尾を超えて鳴っている");

    plugin.deactivate(processor.stop_processing());
}

/// スロット番号なしの綴り（cache-player を入れたときの形）が、
/// 従来どおり note 60 で鳴ること。
#[test]
fn a_bare_path_state_still_sounds_on_note_sixty() {
    let path = write_test_wav("cmrt_cache_player_bare.wav", 64, 0.5, -0.25);

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);
    load_state(&mut plugin, path.to_str().unwrap());

    let mut processor = start_processing(&mut plugin);
    let mut output_events = EventBuffer::with_capacity(8);
    let mut inputs_descriptors = AudioPorts::with_capacity(0, 0);
    let mut outputs_descriptors = AudioPorts::with_capacity(2, 1);

    let sounded = process_block(
        &mut processor,
        &mut inputs_descriptors,
        &mut outputs_descriptors,
        &mut output_events,
        TestNotes::Midi(&[60]),
    );

    assert_eq!(sounded[0][0], 0.5);
    assert_eq!(sounded[1][0], -0.25);

    plugin.deactivate(processor.stop_processing());
}
