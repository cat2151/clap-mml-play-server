//! instance 群の fadeout 要求。

use super::*;

#[test]
fn fade_out_carries_its_instances_and_length() {
    let port = test_port(15);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();
    let last = u8::try_from(MAX_INSTANCE_COUNT - 1).unwrap();

    client.fade_out_instances(&[0, last], 50).unwrap();
    client.fade_out_instances(&[3], MAX_FADE_OUT_MS).unwrap();

    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::FadeOutInstances {
            instance_ids: vec![0, last],
            fade_ms: 50,
        })
    );
    assert_eq!(
        server.recv_timeout(Duration::from_secs(1)).unwrap(),
        Some(FastMidiCommand::FadeOutInstances {
            instance_ids: vec![3],
            fade_ms: MAX_FADE_OUT_MS,
        })
    );
}

#[test]
fn invalid_fade_out_is_rejected_before_enqueue() {
    let port = test_port(16);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();
    let out_of_range = u8::try_from(MAX_INSTANCE_COUNT).unwrap();

    assert!(matches!(
        client.fade_out_instances(&[], 50),
        Err(FastIpcError::InvalidPayload(_))
    ));
    assert!(matches!(
        client.fade_out_instances(&[0], 0),
        Err(FastIpcError::InvalidPayload(_))
    ));
    assert!(matches!(
        client.fade_out_instances(&[0], MAX_FADE_OUT_MS + 1),
        Err(FastIpcError::InvalidPayload(_))
    ));
    assert!(matches!(
        client.fade_out_instances(&[0, out_of_range], 50),
        Err(FastIpcError::InvalidInstance { .. })
    ));
    assert_eq!(server.recv_timeout(Duration::from_millis(1)).unwrap(), None);
}
