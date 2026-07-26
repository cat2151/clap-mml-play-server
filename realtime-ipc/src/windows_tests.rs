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
    client.set_buffer_multiplier(16).unwrap();
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
        Some(FastMidiCommand::SetBufferMultiplier { multiplier: 16 })
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

    assert!(matches!(
        client.stop(16),
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
