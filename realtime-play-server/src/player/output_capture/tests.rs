use super::*;
use std::{sync::mpsc, time::Duration};

fn capture(frames: usize) -> (OutputCapture, mpsc::Receiver<Vec<f32>>) {
    let (tx, rx) = mpsc::channel();
    let capture = OutputCapture::with_writer(frames, move |samples| {
        let _ = tx.send(samples);
    })
    .unwrap();
    (capture, rx)
}

#[test]
fn recording_starts_at_the_first_playing_frame_and_hands_off_when_full() {
    let (mut c, rx) = capture(3);
    c.record(false, 9.0, 9.0);
    c.record(true, 0.1, 0.2);
    c.record(false, 0.0, 0.0);
    assert!(rx.try_recv().is_err(), "満ちるまでは渡さない");
    c.record(true, 0.3, 0.4);
    c.record(true, 5.0, 5.0);

    let written = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(written, vec![0.1, 0.2, 0.0, 0.0, 0.3, 0.4]);
}

#[test]
fn nothing_is_handed_off_before_playback_starts() {
    let (mut c, rx) = capture(1);
    for _ in 0..10 {
        c.record(false, 1.0, 1.0);
    }
    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
}

#[test]
fn recording_never_grows_past_the_reserved_capacity() {
    let (mut c, _rx) = capture(4);
    let reserved = c.samples.capacity();
    for _ in 0..3 {
        c.record(true, 0.5, 0.5);
    }
    assert_eq!(c.samples.capacity(), reserved);
}
