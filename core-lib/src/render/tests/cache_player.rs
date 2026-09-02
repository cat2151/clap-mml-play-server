//! 組み込み cache-player（`.clap` ファイルを持たない静的リンクのプラグイン）を、
//! 実プラグインと**同じ経路**で鳴らす。
//!
//! ここだけ `#[ignore]` が付かない。プラグイン本体がバイナリの中にあるので、
//! 環境変数もインストールも要らずに `cargo test` で走る。

use super::*;
use crate::cache_wav::is_cache_wav_patch_path;
use crate::host::load_builtin_entry;
use cmrt_cache_player::{CachePlayerEntry, CACHE_PLAYER_PLUGIN_ID};

/// 1 秒ぶんの「無音でない」テスト用 WAV を書き、そのパスを返す。
fn write_test_wav(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("cmrt-cache-player-render-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..SAMPLE_RATE as usize {
        let value = (frame as f32 * 0.01).sin() * 0.5;
        writer.write_sample(value).unwrap();
        writer.write_sample(value).unwrap();
    }
    writer.finalize().unwrap();
    path
}

fn cache_player_renderer() -> RealtimeRenderer {
    let entry = load_builtin_entry::<CachePlayerEntry>().unwrap();
    RealtimeRenderer::new(&test_config_with_plugin_id(CACHE_PLAYER_PLUGIN_ID), &entry).unwrap()
}

/// `.clap` ファイルを 1 つも置かずに entry が読め、descriptor まで通ること。
#[test]
fn the_builtin_entry_loads_without_a_clap_file_on_disk() {
    let entry = load_builtin_entry::<CachePlayerEntry>().unwrap();
    let descriptor = select_descriptor(&entry, Some(CACHE_PLAYER_PLUGIN_ID)).unwrap();

    assert_eq!(descriptor.id, CACHE_PLAYER_PLUGIN_ID);
}

/// 擬似 `plugin_path`（`builtin:<id>`）でも同じ entry が読めること。
///
/// `create_live_renderers` も予備プールの `builder` も `load_entry(plugin_path)` しか
/// 呼ばないので、ここが通らないと live 経路へ載らない。
#[test]
fn the_builtin_plugin_path_routes_to_the_same_entry() {
    let path = crate::builtin_plugin_path(CACHE_PLAYER_PLUGIN_ID);
    let entry = load_entry(&path).unwrap();

    assert_eq!(
        select_descriptor(&entry, Some(CACHE_PLAYER_PLUGIN_ID))
            .unwrap()
            .id,
        CACHE_PLAYER_PLUGIN_ID
    );
}

#[test]
fn an_unknown_builtin_id_is_an_error_not_a_dll_load_attempt() {
    let Err(error) = load_entry("builtin:org.example.nope") else {
        panic!("知らない組み込み ID が読み込めてしまった");
    };

    assert!(error.to_string().contains("org.example.nope"), "{error}");
}

/// `set_patch()` に `.wav` を渡すと音源が載り、**MIDI dialect の note on** で鳴ること。
///
/// play server は live も offline も `clap_event_midi`（生の 3 バイト）でノートを送る。
/// CLAP note event だけを見ていると、ホストからは「イベントを送ったのに無音」に見える
/// ——実際にスパイクでそう見えた。ここはその回帰テスト。
#[test]
fn a_wav_patch_sounds_through_the_normal_set_patch_and_midi_note_path() {
    let wav = write_test_wav("sounds.wav");
    let display = wav.to_str().unwrap().to_string();
    assert!(is_cache_wav_patch_path(&display));

    let mut renderer = cache_player_renderer();
    renderer.set_patch(Some(&display)).unwrap();

    let samples = render_live_note(&mut renderer);

    assert!(peak(&samples) > 0.0, "無音になった: {display}");
}

/// 音源を載せていないうちは無音であること（前の音源が残らないことの裏返し）。
#[test]
fn nothing_sounds_before_a_wav_is_loaded() {
    let mut renderer = cache_player_renderer();

    let samples = render_live_note(&mut renderer);

    assert_eq!(peak(&samples), 0.0);
}

/// 実在しない `.wav` は、プラグインまで運ばずパス付きで断ること。
#[test]
fn a_missing_wav_is_rejected_with_the_path() {
    let mut renderer = cache_player_renderer();

    let error = renderer
        .set_patch(Some("N:/does/not/exist.wav"))
        .unwrap_err();

    assert!(error.to_string().contains("exist.wav"), "{error}");
}
