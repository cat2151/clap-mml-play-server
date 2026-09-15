use super::*;

/// テストごとに違う書き出し先。ワークスペースに tempfile を入れていないので、
/// 既存のテスト（`cache-player/src/tests/harness.rs`）と同じく temp_dir を直に使う。
fn out_path(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("cmrt-live-capture-{name}.wav"));
    let _ = std::fs::remove_file(&path);
    path
}

/// `from_env` は env に触るので、ここでは構造体を直に組んで振る舞いだけ見る。
fn capture(path: &std::path::Path, seconds: f64, sample_rate: u32) -> LiveCapture {
    let capacity = (seconds * sample_rate as f64).round() as usize * 2;
    LiveCapture {
        path: path.to_path_buf(),
        sample_rate,
        samples: Vec::with_capacity(capacity),
        first_clock: None,
        truncated: false,
        minimum_frames: 0,
        written: false,
    }
}

#[test]
fn a_capture_keeps_every_block_in_order() {
    let mut c = capture(&out_path("in-order"), 1.0, 48_000);
    c.push(&[0.1, 0.2], 0);
    c.push(&[0.3, 0.4], 1);
    assert_eq!(c.samples, vec![0.1, 0.2, 0.3, 0.4]);
    assert_eq!(c.first_clock, Some(0));
}

#[test]
fn a_capture_never_grows_past_its_reserved_capacity() {
    // 2 フレーム＝4 要素ぶんだけ確保する。
    let mut c = capture(&out_path("capacity"), 2.0, 1);
    let before = c.samples.capacity();
    c.push(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 0);
    assert_eq!(
        c.samples.capacity(),
        before,
        "render スレッドで再確保している"
    );
    assert_eq!(c.samples, vec![0.1, 0.2, 0.3, 0.4]);
    assert!(c.truncated);
}

#[test]
fn a_finished_capture_writes_the_wav_once() {
    let path = out_path("write-once");
    let mut c = capture(&path, 1.0, 48_000);
    c.push(&[0.25, -0.25], 512);
    c.finish();
    assert!(path.is_file());
    let len = std::fs::metadata(&path).unwrap().len();

    // 2 回目は書かない（停止のたびに呼ばれるため）。
    c.push(&[0.5, 0.5], 1024);
    c.finish();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), len);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_empty_capture_writes_nothing() {
    let path = out_path("empty");
    let mut c = capture(&path, 1.0, 48_000);
    c.finish();
    assert!(!path.exists());
}

#[test]
fn stop_does_not_finish_before_the_requested_minimum_length() {
    let path = out_path("minimum-length");
    let mut c = capture(&path, 2.0, 10);
    c.minimum_frames = 10;
    c.push(&[0.25; 16], 0);

    c.finish_on_stop();

    assert!(!c.written);
    assert!(!path.exists());
    c.push(&[0.5; 4], 8);
    c.finish_on_stop();
    assert!(c.written);
    assert!(path.exists());
    let _ = std::fs::remove_file(path);
}
