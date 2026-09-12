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
///
/// `amplitude` を変えた 2 本を作れば、**どちらが鳴ったか**を peak で見分けられる。
/// 「鳴った / 鳴らない」だけでは、全スロットを鳴らす実装でもテストが通ってしまう。
fn write_test_wav(name: &str, amplitude: f32) -> std::path::PathBuf {
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
        let value = (frame as f32 * 0.01).sin() * amplitude;
        writer.write_sample(value).unwrap();
        writer.write_sample(value).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// note number を指定して [`render_live_note`] と同じ手順を踏む。
///
/// cache-player は **note number でスロットを選ぶ**ので、60 決め打ちの
/// [`render_live_note`] ではスロット 1 に載せた WAV を鳴らせない。
fn render_live_note_number(renderer: &mut RealtimeRenderer, note: u8) -> Vec<f32> {
    let mut samples = renderer.render_live_chunk(&[[0x90, note, 100]]).unwrap();
    for _ in 0..9 {
        samples.extend(renderer.render_live_chunk(&[]).unwrap());
    }
    samples.extend(renderer.render_live_chunk(&[[0x80, note, 0]]).unwrap());
    samples
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
    let wav = write_test_wav("sounds.wav", 0.5);
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
        .set_patch(Some("X:/does/not/exist.wav"))
        .unwrap_err();

    assert!(error.to_string().contains("exist.wav"), "{error}");
}

/// スロット指定つきの patch 文字列が、**指定したスロットへ**載ること。
///
/// スロット 1 へ載せたのだから、スロット 0 を指す note 60 では鳴らない。
/// 「鳴った」だけを見るテストだと、スロット指定を無視して 0 へ載せる実装でも通る。
#[test]
fn a_slot_specified_patch_lands_on_that_slot_only() {
    let wav = write_test_wav("slot_one.wav", 0.5);
    let patch = crate::cache_wav::cache_wav_patch_with_slot(1, wav.to_str().unwrap());
    assert!(is_cache_wav_patch_path(&patch), "{patch}");

    let mut renderer = cache_player_renderer();
    renderer.set_patch(Some(&patch)).unwrap();

    assert_eq!(
        peak(&render_live_note_number(&mut renderer, 60)),
        0.0,
        "スロット 0 は空のままのはず: {patch}"
    );
    assert!(
        peak(&render_live_note_number(&mut renderer, 61)) > 0.0,
        "スロット 1 が鳴らない: {patch}"
    );
}

/// 2 つの小節ぶんを**同時に**載せておけること（先読みが成り立つ条件）。
///
/// 片方を載せてももう片方が消えないこと、そして note number が
/// **自分のスロットの WAV** を鳴らすことを、振幅の違いで見分ける。
#[test]
fn both_slots_stay_loaded_and_each_note_sounds_its_own_wav() {
    let loud = write_test_wav("slot_zero_loud.wav", 0.5);
    let quiet = write_test_wav("slot_one_quiet.wav", 0.125);
    let patches = [
        crate::cache_wav::cache_wav_patch_with_slot(0, loud.to_str().unwrap()),
        crate::cache_wav::cache_wav_patch_with_slot(1, quiet.to_str().unwrap()),
    ];

    // 鳴らし始めた voice は WAV の最後まで残る（余韻を切らないための仕様）ので、
    // 2 つの note を同じ renderer で続けて鳴らすと peak が混ざる。note ごとに作り直す。
    let peak_for = |note: u8| {
        let mut renderer = cache_player_renderer();
        for patch in &patches {
            renderer.set_patch(Some(patch)).unwrap();
        }
        peak(&render_live_note_number(&mut renderer, note))
    };

    let slot_zero = peak_for(60);
    let slot_one = peak_for(61);

    assert!(slot_zero > 0.4, "スロット 0 が消えた: {slot_zero}");
    assert!(slot_one > 0.0, "スロット 1 が鳴らない: {slot_one}");
    assert!(
        slot_zero > slot_one * 2.0,
        "note ごとに別の WAV が鳴っていない: slot0={slot_zero} slot1={slot_one}"
    );
}

/// スロット番号なしの綴りは従来どおりスロット 0（＝note 60）で鳴ること。
///
/// cache-player を入れたときの呼び出しをそのまま残すための後方互換。
#[test]
fn a_bare_path_still_lands_in_slot_zero() {
    let wav = write_test_wav("bare_path.wav", 0.5);
    let display = wav.to_str().unwrap().to_string();

    let mut renderer = cache_player_renderer();
    renderer.set_patch(Some(&display)).unwrap();

    assert_eq!(
        peak(&render_live_note_number(&mut renderer, 61)),
        0.0,
        "スロット 1 まで載ってしまった: {display}"
    );
    assert!(
        peak(&render_live_note_number(&mut renderer, 60)) > 0.0,
        "スロット 0 で鳴らない: {display}"
    );
}

/// 壊れたスロット番号は、プラグインまで運ばずここで断ること。
#[test]
fn a_broken_slot_number_is_rejected_before_the_plugin() {
    let mut renderer = cache_player_renderer();

    let error = renderer.set_patch(Some("slot=9;C:/x.wav")).unwrap_err();

    assert!(error.to_string().contains("slot=9"), "{error}");
}

/// 位置が値に書いてある WAV を書く。**値からそのまま再生位置（フレーム番号）が読める。**
///
/// 「鳴った / 鳴らない」しか見ないテストでは、再生位置が飛んでも通ってしまう。
fn write_ramp_wav(name: &str, frames: usize) -> std::path::PathBuf {
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
    for frame in 0..frames {
        let value = frame as f32 * RAMP_STEP;
        writer.write_sample(value).unwrap();
        writer.write_sample(value).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// ramp の 1 フレームあたりの増分。f32 でも丸めずに元のフレーム番号へ戻せる大きさ。
const RAMP_STEP: f32 = 1.0 / 48_000.0;

/// ステレオ interleave の先頭サンプルから、鳴っている音源のフレーム番号を読む。
fn ramp_position(samples: &[f32]) -> usize {
    (samples[0] / RAMP_STEP).round() as usize
}

/// **鳴っている最中に次の小節を先読みしても、再生位置が飛ばないこと。**
///
/// play server の先読み（`PrepareLivePatch`）は「鳴っている音を切る → state load →
/// 反映のため空回し」という手順で、同じ instance の**別スロット**へ次の小節を載せる。
/// その下準備は `process()` を呼ぶので、cache-player のように鳴っている voice を
/// 切らないプラグインでは、その voice の再生位置が空回しぶん先へ飛ぶ。
///
/// 直す前は reset と settle の空回しで 2560 フレーム（53ms）飛んでいた
/// （`docs/adr/0018-patch-load-must-not-spin-the-plugin.md`）。ここはその回帰テスト。
#[test]
fn a_prefetch_while_sounding_does_not_advance_the_playing_voice() {
    let playing = write_ramp_wav("prefetch_playing_ramp.wav", SAMPLE_RATE as usize);
    let next = write_test_wav("prefetch_next_measure.wav", 0.25);
    let playing_patch = crate::cache_wav::cache_wav_patch_with_slot(0, playing.to_str().unwrap());
    let next_patch = crate::cache_wav::cache_wav_patch_with_slot(1, next.to_str().unwrap());

    let mut renderer = cache_player_renderer();
    renderer.set_patch(Some(&playing_patch)).unwrap();
    // 小節の頭の note on（スロット 0 = note 60）。
    let first = renderer.render_live_chunk(&[[0x90, 60, 100]]).unwrap();
    assert_eq!(ramp_position(&first), 0, "note on の位置から鳴っていない");
    let second = renderer.render_live_chunk(&[]).unwrap();
    assert_eq!(ramp_position(&second), BUFFER_SIZE);

    // ここで次の小節を先読みする。play server の `prepare_patch` と同じ手順。
    renderer
        .switch_patch(Some(&next_patch), true, PATCH_SETTLE_BLOCKS)
        .unwrap();

    let after = renderer.render_live_chunk(&[]).unwrap();
    assert_eq!(
        ramp_position(&after),
        BUFFER_SIZE * 2,
        "先読みで再生位置が {} フレーム飛んだ",
        ramp_position(&after) as i64 - (BUFFER_SIZE * 2) as i64
    );
}

/// play server が先読みで空回しするブロック数（`worker/bank/state.rs` と同じ値）。
///
/// **飛ぶ量はこの数で決まる**ので、テスト側にも同じ値を持たせて再現させる。
const PATCH_SETTLE_BLOCKS: usize = 4;

/// cache-player は「差し替えても鳴っている音を切らない」側であること。
///
/// この判定が裏返ると、上のテストが守っているものが丸ごと無くなる。
#[test]
fn the_cache_player_is_a_plugin_that_keeps_its_voices_across_a_patch_load() {
    let renderer = cache_player_renderer();

    assert!(renderer.keeps_voices_across_patch_load());
}
