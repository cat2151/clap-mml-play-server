use super::*;
use std::time::Duration;

fn test_port(offset: u16) -> u16 {
    30_000 + ((std::process::id() as u16).wrapping_add(offset) % 20_000)
}

#[test]
fn round_trip_preserves_multi_instance_events_and_controls() {
    let port = test_port(0);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();
    client
        .send_events(&[
            FastMidiEvent {
                instance_id: 0,
                offset_frames: 0,
                message: [0x90, 60, 100],
            },
            FastMidiEvent {
                instance_id: 15,
                offset_frames: 5538,
                message: [0x80, 72, 0],
            },
        ])
        .unwrap();
    // 倍率は wire format 上 u32 なので、u8 に収まらない上限でも運べる。
    client.set_buffer_multiplier(MAX_BUFFER_MULTIPLIER).unwrap();
    client.set_auto_gain_enabled(true).unwrap();
    client.stop(15).unwrap();
    client.stop_all().unwrap();

    assert!(matches!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::Midi { events }) if events.len() == 2
            && events[0].instance_id == 0
            && events[1].instance_id == 15
            && events[1].offset_frames == 5538
    ));
    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::SetBufferMultiplier {
            multiplier: MAX_BUFFER_MULTIPLIER
        })
    );
    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::SetAutoGain { enabled: true })
    );
    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::Stop { instance_id: 15 })
    );
    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::StopAll)
    );
}

#[test]
fn prepare_patch_waits_for_success_response() {
    let port = test_port(1);
    let mut server = FastMidiServer::create(port).unwrap();
    let server_thread = std::thread::spawn(move || {
        let command = server
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let FastMidiCommand::PreparePatch {
            request_id,
            instance_id,
            patch,
            probe,
        } = command
        else {
            panic!("unexpected command");
        };
        assert_eq!(instance_id, 7);
        assert_eq!(patch.as_deref(), Some("Keys/Piano.fxp"));
        assert!(!probe);
        server.complete_request(request_id, Ok(&[])).unwrap();
    });
    let mut client = FastMidiClient::connect(port).unwrap();

    client.prepare_patch(7, Some("Keys/Piano.fxp")).unwrap();
    server_thread.join().unwrap();
}

#[test]
fn probe_and_error_responses_are_returned_to_the_client() {
    let port = test_port(2);
    let mut server = FastMidiServer::create(port).unwrap();
    let server_thread = std::thread::spawn(move || {
        for result in [Ok(br#"{"decision":"poly"}"#.as_slice()), Err("load failed")] {
            let FastMidiCommand::PreparePatch { request_id, .. } = server
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap()
            else {
                panic!("unexpected command");
            };
            server.complete_request(request_id, result).unwrap();
        }
    });
    let mut client = FastMidiClient::connect(port).unwrap();

    assert_eq!(
        client.probe_patch(0, None).unwrap(),
        br#"{"decision":"poly"}"#
    );
    assert!(matches!(
        client.prepare_patch(0, None),
        Err(FastIpcError::RequestFailed(message)) if message == "load failed"
    ));
    server_thread.join().unwrap();
}

#[test]
fn invalid_instance_is_rejected_before_enqueue() {
    let port = test_port(3);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();

    // 上限ちょうどの id は範囲外。定数から導いて、上限を動かしてもテストが空振りしないようにする。
    let out_of_range = u8::try_from(crate::MAX_INSTANCE_COUNT).unwrap();
    assert!(matches!(
        client.stop(out_of_range),
        Err(FastIpcError::InvalidInstance { .. })
    ));
    assert_eq!(server.recv_timeout(Duration::from_millis(1)).unwrap(), None);
}

#[test]
fn limiter_meter_peak_is_reset_when_read() {
    let port = test_port(4);
    let server = FastMidiServer::create(port).unwrap();
    let client = FastMidiClient::connect(port).unwrap();
    server.publish_limiter_meter(LimiterMeter {
        current_reduction_db: 2.0,
        peak_reduction_db: 4.5,
    });

    assert_eq!(
        client.limiter_meter(),
        LimiterMeter {
            current_reduction_db: 2.0,
            peak_reduction_db: 4.5,
        }
    );
    assert_eq!(client.limiter_meter().peak_reduction_db, 0.0);
}

#[test]
fn underrun_frames_are_published_as_a_monotonic_snapshot() {
    let port = test_port(5);
    let server = FastMidiServer::create(port).unwrap();
    let client = FastMidiClient::connect(port).unwrap();

    server.publish_underrun_frames(1_234_567_890_123);

    assert_eq!(client.underrun_frames(), 1_234_567_890_123);
    assert_eq!(client.underrun_frames(), 1_234_567_890_123);
}

/// 渡さなかった instance は 0 dB へ戻ること。track 数を減らしたあと、消えた行の
/// 古い値が残り続けると「鳴っていないのに +3dB」に見えてしまう。
#[test]
fn auto_gain_db_is_published_per_instance_and_cleared_past_the_end() {
    let port = test_port(7);
    let server = FastMidiServer::create(port).unwrap();
    let client = FastMidiClient::connect(port).unwrap();

    server.publish_auto_gain_db(&[3.0, -1.5, 0.0]);
    let gains = client.auto_gain_db();
    assert_eq!(&gains[..3], &[3.0, -1.5, 0.0]);
    assert!(gains[3..].iter().all(|gain| *gain == 0.0));

    server.publish_auto_gain_db(&[1.0]);
    let gains = client.auto_gain_db();
    assert_eq!(gains[0], 1.0);
    assert!(gains[1..].iter().all(|gain| *gain == 0.0));
}

#[test]
fn second_client_is_rejected_until_first_drops() {
    let port = test_port(6);
    let _server = FastMidiServer::create(port).unwrap();
    let first = FastMidiClient::connect(port).unwrap();
    assert!(matches!(
        FastMidiClient::connect(port),
        Err(FastIpcError::AlreadyConnected)
    ));
    drop(first);
    assert!(FastMidiClient::connect(port).is_ok());
}

#[test]
fn absolute_timeline_round_trip_preserves_f64_bits() {
    let port = test_port(8);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();
    let config = LiveTimelineConfig {
        timeline_id: 99,
        sample_rate_hz: 48_000.0,
        tempo_bpm: 130.0,
        time_signature_numerator: 4,
        time_signature_denominator: 4,
    };
    let at = 31_199.0 * 60.0 / 520.0;
    client.begin_live_timeline(config).unwrap();
    client
        .send_timeline_events(&[TimelineMidiEvent {
            timeline_id: 99,
            instance_id: 3,
            timeline_seconds: at,
            message: [0x90, 64, 100],
        }])
        .unwrap();

    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::BeginLiveTimeline(config))
    );
    let Some(FastMidiCommand::TimelineMidi { events }) =
        server.recv_timeout(Duration::from_secs(1)).unwrap()
    else {
        panic!("expected timeline MIDI");
    };
    assert_eq!(events[0].timeline_seconds.to_bits(), at.to_bits());
    assert_eq!(events[0].timeline_id, 99);
}

#[test]
fn live_tempo_changes_round_trip_and_reject_invalid_values() {
    let port = test_port(10);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();
    // grid が1周する秒（BPM130 の 16 ステップ）。丸めずにビットのまま運ぶこと。
    let at = 16.0 * 60.0 / 520.0;
    let change = LiveTempoChange {
        timeline_id: 42,
        at_seconds: at,
        tempo_bpm: 83.5,
        time_signature_numerator: 3,
        time_signature_denominator: 8,
    };
    client.set_live_tempo(change).unwrap();

    let Some(FastMidiCommand::SetLiveTempo(received)) =
        server.recv_timeout(Duration::from_secs(1)).unwrap()
    else {
        panic!("expected a live tempo change");
    };
    assert_eq!(received, change);
    assert_eq!(received.at_seconds.to_bits(), at.to_bits());

    for invalid in [
        LiveTempoChange {
            timeline_id: 0,
            ..change
        },
        LiveTempoChange {
            at_seconds: -1.0,
            ..change
        },
        LiveTempoChange {
            at_seconds: f64::NAN,
            ..change
        },
        LiveTempoChange {
            tempo_bpm: 0.0,
            ..change
        },
        LiveTempoChange {
            time_signature_denominator: 3,
            ..change
        },
    ] {
        assert!(
            matches!(
                client.set_live_tempo(invalid),
                Err(FastIpcError::InvalidPayload(_))
            ),
            "{invalid:?} が通ってしまった"
        );
    }
    // 弾いたものは1件も届いていない。
    assert_eq!(
        server.recv_timeout(Duration::from_millis(50)).unwrap(),
        None
    );
}

#[test]
fn timing_metrics_are_published_as_one_snapshot() {
    let port = test_port(9);
    let server = FastMidiServer::create(port).unwrap();
    let client = FastMidiClient::connect(port).unwrap();
    let expected = TimingMetrics {
        events: 120,
        late_events: 2,
        late_events_total: 7,
        max_late_samples: 48,
        max_late_us: 1_000.0,
        output_lead_min_frames: 512,
        output_lead_max_frames: 2_048,
        process_load_p95: 42.0,
        process_load_max: 87.5,
    };
    server.publish_timing_metrics(expected);
    assert_eq!(client.timing_metrics(), expected);
}
