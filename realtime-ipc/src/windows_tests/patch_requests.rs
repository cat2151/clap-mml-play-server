//! 音色の準備・先読み・probe の要求と応答。

use super::*;

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
            effect_chain,
            probe,
        } = command
        else {
            panic!("unexpected command");
        };
        assert_eq!(instance_id, 7);
        assert_eq!(patch.as_deref(), Some("Keys/Piano.fxp"));
        assert_eq!(effect_chain, "", "chain 無しの method は空で届く");
        assert!(!probe);
        server.complete_request(request_id, Ok(&[])).unwrap();
    });
    let mut client = FastMidiClient::connect(port).unwrap();

    client.prepare_patch(7, Some("Keys/Piano.fxp")).unwrap();
    server_thread.join().unwrap();
}

/// 先読みは `PreparePatch` と別のコマンドとして届くこと。
///
/// 「非演奏 bank への先読みだからレンダーを止めてよい」という判断は、
/// サーバーがこの区別を受け取れて初めて成り立つ。同じ KIND に混ぜると、
/// 現在 bank の行音色変更まで巻き込んで無音になる。
#[test]
fn standby_preload_arrives_as_its_own_command_kind() {
    let port = test_port(11);
    let mut server = FastMidiServer::create(port).unwrap();
    let server_thread = std::thread::spawn(move || {
        let command = server
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let FastMidiCommand::PrepareStandbyPatch {
            request_id,
            instance_id,
            patch,
            ..
        } = command
        else {
            panic!("unexpected command: {command:?}");
        };
        assert_eq!(instance_id, 9);
        assert_eq!(patch.as_deref(), Some("Keys/Piano.fxp"));
        server.complete_request(request_id, Ok(&[])).unwrap();

        // 続く1件は失敗応答。呼び出し側へそのまま伝わること。
        let FastMidiCommand::PrepareStandbyPatch { request_id, .. } = server
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap()
        else {
            panic!("unexpected command");
        };
        server
            .complete_request(request_id, Err("standby load failed"))
            .unwrap();
    });
    let mut client = FastMidiClient::connect(port).unwrap();

    client
        .prepare_standby_patch(9, Some("Keys/Piano.fxp"))
        .unwrap();
    assert!(matches!(
        client.prepare_standby_patch(9, None),
        Err(FastIpcError::RequestFailed(message)) if message == "standby load failed"
    ));
    server_thread.join().unwrap();
}

/// 音色の準備に同梱した chain が、通常の準備と先読みのどちらでもそのまま届くこと。
#[test]
fn effect_chain_travels_with_both_patch_requests() {
    const CHAIN: &str = r#"[{"Surge XT Effects preset": "Reverb 1/Cathedral 2"}]"#;
    let port = test_port(14);
    let mut server = FastMidiServer::create(port).unwrap();
    let server_thread = std::thread::spawn(move || {
        let Some(FastMidiCommand::PreparePatch {
            request_id,
            patch,
            effect_chain,
            ..
        }) = server.recv_timeout(Duration::from_secs(1)).unwrap()
        else {
            panic!("unexpected command");
        };
        assert_eq!(patch.as_deref(), Some("Keys/Piano.fxp"));
        assert_eq!(effect_chain, CHAIN);
        server.complete_request(request_id, Ok(&[])).unwrap();

        let Some(FastMidiCommand::PrepareStandbyPatch {
            request_id,
            patch,
            effect_chain,
            ..
        }) = server.recv_timeout(Duration::from_secs(1)).unwrap()
        else {
            panic!("unexpected command");
        };
        assert_eq!(patch, None);
        assert_eq!(effect_chain, CHAIN);
        server.complete_request(request_id, Ok(&[])).unwrap();
    });
    let mut client = FastMidiClient::connect(port).unwrap();

    client
        .prepare_patch_with_effect_chain(3, Some("Keys/Piano.fxp"), CHAIN)
        .unwrap();
    client
        .begin_standby_patch_with_effect_chain(4, None, CHAIN)
        .unwrap();
    server_thread.join().unwrap();

    let too_long = "x".repeat(MAX_EFFECT_CHAIN_BYTES + 1);
    assert_eq!(
        client.prepare_patch_with_effect_chain(3, None, &too_long),
        Err(FastIpcError::EffectChainTooLong {
            bytes: MAX_EFFECT_CHAIN_BYTES + 1,
            max: MAX_EFFECT_CHAIN_BYTES,
        })
    );
}

/// 範囲外の instance は送信前に弾くこと（`prepare_patch` と同じ扱い）。
#[test]
fn standby_preload_rejects_out_of_range_instances_before_enqueue() {
    let port = test_port(12);
    let mut server = FastMidiServer::create(port).unwrap();
    let mut client = FastMidiClient::connect(port).unwrap();

    let out_of_range = u8::try_from(crate::MAX_INSTANCE_COUNT).unwrap();
    assert!(matches!(
        client.prepare_standby_patch(out_of_range, None),
        Err(FastIpcError::InvalidInstance { .. })
    ));
    assert_eq!(server.recv_timeout(Duration::from_millis(1)).unwrap(), None);
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
