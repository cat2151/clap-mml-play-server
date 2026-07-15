use super::*;

fn test_port(offset: u16) -> u16 {
    30_000 + ((unsafe { GetCurrentProcessId() } as u16).wrapping_add(offset) % 20_000)
}

#[test]
fn round_trip_preserves_midi_patch_buffer_multiplier_and_stop() {
    let port = test_port(0);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();

    client
        .send_midi(&[[0x90, 60, 100], [0x80, 60, 0]], Some("Keys/Piano.fxp"))
        .unwrap();
    client.set_buffer_multiplier(8).unwrap();
    client.stop().unwrap();

    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::Midi {
            messages: vec![[0x90, 60, 100], [0x80, 60, 0]],
            patch: Some("Keys/Piano.fxp".to_string()),
        })
    );
    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::SetBufferMultiplier { multiplier: 8 })
    );
    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::Stop)
    );
}

#[test]
fn invalid_buffer_multiplier_is_rejected_before_enqueue() {
    let port = test_port(4);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();

    assert!(matches!(
        client.set_buffer_multiplier(3),
        Err(FastIpcError::InvalidPayload(_))
    ));
    assert_eq!(server.recv_timeout(Duration::from_millis(1)).unwrap(), None);
}

#[test]
fn second_client_is_rejected_until_first_drops() {
    let port = test_port(1);
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
fn invalid_midi_is_rejected_before_enqueue() {
    let port = test_port(3);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();

    assert!(matches!(
        client.send_midi(&[[0x90, 0x80, 100]], None),
        Err(FastIpcError::InvalidPayload(_))
    ));
    assert_eq!(server.recv_timeout(Duration::from_millis(1)).unwrap(), None);
}

#[test]
fn server_restart_invalidates_old_client_ownership() {
    let port = test_port(2);
    let server = FastMidiServer::create(port).unwrap();
    let mut old_client = FastMidiClient::connect(port).unwrap();
    drop(server);

    let mut restarted_server = FastMidiServer::create(port).unwrap();
    assert!(matches!(
        old_client.send_midi(&[[0x90, 60, 100]], None),
        Err(FastIpcError::ServerStopped)
    ));
    drop(old_client);

    let mut new_client = FastMidiClient::connect(port).unwrap();
    new_client.send_midi(&[[0x90, 64, 100]], None).unwrap();
    assert_eq!(
        restarted_server
            .recv_timeout(Duration::from_secs(1))
            .unwrap(),
        Some(FastMidiCommand::Midi {
            messages: vec![[0x90, 64, 100]],
            patch: None,
        })
    );
}
