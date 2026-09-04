//! **書き込み途中のキャッシュ WAV を読むと何が起きるか**（判定用。直しではない）。
//!
//! # なぜここを見るのか
//! DAW のキャッシュ WAV は `cmrt_core::write_wav`（`core-lib/src/pipeline/audio.rs`）が
//! **最終パスを直接 truncate して書く**。一時ファイル + rename ではない。一方 DAW の
//! 演奏ループはファイルが在るかしか見ずに `PreparePatch` を出すので
//! （`clap-mml-render-tui` の `docs/adr/0018-page-replacement-clears-the-cache.md`）、
//! **書いている最中のファイルを読む窓が実在する。**
//!
//! # 窓に見えるのは 2 つの状態だけ
//! `WavWriter::create` は `File::create`（0 バイトへ truncate）→ `BufWriter` →
//! ヘッダのサイズ欄を **0 で埋めて**書き出す、の順で始まり、サイズ欄が埋まるのは
//! `finalize()` のときだけ（hound 3.5.1 `write.rs` の `write_headers` / `update_header`）。
//! だからディスクに見えるのは
//!
//! 1. **0 バイト**（`File::create` 直後。ヘッダはまだ `BufWriter` の中）
//! 2. **ヘッダ + 途中までの本体。ただし data チャンク長は 0**（`BufWriter` が溢れて
//!    吐き出したあと、`finalize()` の前）
//!
//! の 2 つ。**前日のファイルの中身が途中まで見える、という状態にはならない**
//! （`File::create` が最初に消してしまうため）。
//!
//! # 判定
//! ここのテストが固定しているのは **いまの（壊れている）振る舞い**。
//! 直したら赤くなる向きに書いてある。

use std::cell::{Cell, RefCell};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::rc::Rc;

use clack_host::prelude::*;

use crate::buffer::CacheBuffer;
use crate::slots::slot_patch_state;
use crate::tests::harness::*;

/// 書きかけの WAV の中身。L/R に定数を入れてあるので、**もし鳴ったら気づける**。
const HALF_WRITTEN_LEFT: f32 = 0.75;
const HALF_WRITTEN_RIGHT: f32 = -0.75;

/// 1 つ前の小節として先にスロットへ載せておく WAV の中身。
const PREVIOUS_MEASURE_LEFT: f32 = 0.5;
const PREVIOUS_MEASURE_RIGHT: f32 = -0.25;

/// `WavWriter` が「書き込み途中に」残すバイト列を、そのまま取り出すための書き込み先。
///
/// **`finalize()` も `Drop` もヘッダを埋めてしまう**ので、途中経過は
/// writer を生かしたまま覗くしかない。`Rc` で中身を外から共有する。
#[derive(Clone, Default)]
struct SharedSink {
    bytes: Rc<RefCell<Vec<u8>>>,
    pos: Rc<Cell<u64>>,
}

impl Write for SharedSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut bytes = self.bytes.borrow_mut();
        let start = self.pos.get() as usize;
        if bytes.len() < start + buf.len() {
            bytes.resize(start + buf.len(), 0);
        }
        bytes[start..start + buf.len()].copy_from_slice(buf);
        self.pos.set((start + buf.len()) as u64);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for SharedSink {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let len = self.bytes.borrow().len() as i64;
        let next = match from {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::End(offset) => len + offset,
            SeekFrom::Current(offset) => self.pos.get() as i64 + offset,
        };
        let next = next.max(0) as u64;
        self.pos.set(next);
        Ok(next)
    }
}

fn cache_wav_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }
}

/// `write_wav` が `frames` フレームまで書いた時点で**ディスクに在るバイト列**を作る。
///
/// 本物の書き手と同じ `hound::WavWriter` に同じ spec で書かせ、`finalize()` の前に
/// 中身を写し取る。だから「ヘッダのサイズ欄が 0 のまま」も本物どおりに再現される。
fn half_written_wav_bytes(frames: usize) -> Vec<u8> {
    let sink = SharedSink::default();
    let mut writer = hound::WavWriter::new(sink.clone(), cache_wav_spec()).unwrap();
    for _ in 0..frames {
        writer.write_sample(HALF_WRITTEN_LEFT).unwrap();
        writer.write_sample(HALF_WRITTEN_RIGHT).unwrap();
    }
    // ここで写す。writer はこのあと Drop でヘッダを埋めるが、写しには影響しない。
    let snapshot = sink.bytes.borrow().clone();
    drop(writer);
    snapshot
}

/// 書きかけのバイト列をそのままファイルへ置く。
fn write_half_written_wav(name: &str, frames: usize) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, half_written_wav_bytes(frames)).unwrap();
    path
}

/// 書き込みが始まった直後（`File::create` が truncate しただけ）のファイル。
fn write_zero_byte_wav(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, []).unwrap();
    path
}

/// **壊れている振る舞いの記録。** 書き込み途中の WAV は、中身が何 MB 入っていても
/// **エラーにならず 0 フレームとして読めてしまう。**
///
/// hound はヘッダの data チャンク長だけを見てサンプル数を決めるので、
/// `finalize()` 前の「長さ 0」がそのまま「音が 1 フレームも無い WAV」になる。
/// 呼び出し側（`CachePlayerMainThread::load`）はこれを成功として扱うので、
/// **誰もおかしいと気づけない。**
///
/// 直して赤くする向き: `load_wav` が「0 フレームの WAV は拒む」または
/// 「data チャンク長 0 かつ本体が在る＝書きかけ」を見るようにすると、
/// `expect` が失敗して赤くなる。
#[test]
fn a_wav_that_is_still_being_written_reads_as_zero_frames_instead_of_failing() {
    let path = write_half_written_wav("cmrt_cache_player_half_written.wav", 12_000);

    let on_disk = std::fs::metadata(&path).unwrap().len();
    assert!(
        on_disk > 90_000,
        "書きかけとはいえ本体は既に {on_disk} バイト在る（＝空ファイルではない）"
    );

    let buffer = CacheBuffer::load_wav(path.to_str().unwrap())
        .expect("書きかけの WAV でも load_wav は成功してしまう（これがいまの振る舞い）");

    assert_eq!(
        buffer.frames(),
        0,
        "ヘッダの data チャンク長が 0 のままなので、本体が在っても 0 フレームに見える"
    );
    assert_eq!(buffer.sample_rate(), 48_000, "fmt チャンクは読めている");
}

/// **壊れている振る舞いの記録。** 書き込み途中の WAV を載せると、
/// **state load は成功したまま、その小節のその行だけが無音になる。**
///
/// サーバーから見て `PreparePatch` は `Ok` なので instance は live mix に残り、
/// DAW 側もその行を note on の対象に残す（`daw/src/playback/live_cache/send.rs` の
/// `prepared`）。**エラーがどこにも出ないまま音だけが消える**のがこの経路の性質。
///
/// 直して赤くする向き: 書きかけを弾くようにすると `try_load_state` が false になり、
/// 1 本目の assert が赤くなる。
#[test]
fn a_measure_whose_wav_is_still_being_written_falls_silent_without_reporting_an_error() {
    let half_written = write_half_written_wav("cmrt_cache_player_half_written_slot.wav", 12_000);

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);

    // 1 つ前の小節をスロット 0 へ載せておく（踏み潰しの有無を見分けるため）。
    let previous = write_test_wav(
        "cmrt_cache_player_previous_measure.wav",
        64,
        PREVIOUS_MEASURE_LEFT,
        PREVIOUS_MEASURE_RIGHT,
    );
    load_state(
        &mut plugin,
        &slot_patch_state(0, previous.to_str().unwrap()),
    );

    let loaded = try_load_state(
        &mut plugin,
        &slot_patch_state(0, half_written.to_str().unwrap()),
    );
    assert!(
        loaded,
        "書きかけの WAV でも state load は成功する（＝サーバーは異常だと気づけない）"
    );

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

    assert!(
        sounded[0].iter().all(|sample| *sample == 0.0),
        "書きかけの WAV は 0 フレームなので、その小節は丸ごと無音になる: {:?}",
        &sounded[0][..8]
    );
    assert!(sounded[1].iter().all(|sample| *sample == 0.0), "R も無音");
    assert_ne!(
        sounded[0][0], PREVIOUS_MEASURE_LEFT,
        "スロットは差し替わっているので、1 つ前の小節が鳴るわけではない"
    );

    plugin.deactivate(processor.stop_processing());
}

/// **窓のもう片方の記録。** 書き込みが始まった直後（0 バイト）のファイルは
/// **state load が失敗し、スロットには 1 つ前の同じ剰余の小節が残ったままになる。**
///
/// つまり「そのまま note on を送れば**別の小節が鳴る**」。DAW 側が失敗した行を
/// note on の対象から外している（`daw/src/playback/live_cache/send.rs` の `prepared`）のは
/// この形を避けるためで、**その防御が効いている限り実害は無音まで**。
///
/// 直して赤くする向き: 失敗時にスロットを空にするようにすると、
/// 最後の assert（1 つ前の小節が鳴る）が赤くなる。
#[test]
fn a_wav_truncated_to_zero_bytes_fails_to_load_and_leaves_the_previous_measure_in_the_slot() {
    let empty = write_zero_byte_wav("cmrt_cache_player_zero_byte.wav");
    assert!(
        CacheBuffer::load_wav(empty.to_str().unwrap()).is_err(),
        "0 バイトのファイルは RIFF が読めないので load_wav は失敗する"
    );

    let entry = load_entry();
    let mut plugin = new_plugin(&entry);

    let previous = write_test_wav(
        "cmrt_cache_player_previous_measure_kept.wav",
        64,
        PREVIOUS_MEASURE_LEFT,
        PREVIOUS_MEASURE_RIGHT,
    );
    load_state(
        &mut plugin,
        &slot_patch_state(0, previous.to_str().unwrap()),
    );

    let loaded = try_load_state(&mut plugin, &slot_patch_state(0, empty.to_str().unwrap()));
    assert!(!loaded, "0 バイトの WAV なら state load は失敗する");

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

    assert_eq!(
        sounded[0][0], PREVIOUS_MEASURE_LEFT,
        "失敗したロードはスロットを触らないので、1 つ前の小節がそのまま鳴る"
    );
    assert_eq!(sounded[1][0], PREVIOUS_MEASURE_RIGHT);

    plugin.deactivate(processor.stop_processing());
}
