use super::*;
use std::{
    io::{Read as _, Write as _},
    sync::Mutex,
};

#[derive(Default)]
struct FakePlayer {
    plays: Mutex<Vec<Vec<u8>>>,
    mml_plays: Mutex<Vec<String>>,
    midi_batches: Mutex<Vec<Vec<[u8; 3]>>>,
    midi_patches: Mutex<Vec<Option<String>>>,
    prepared_live_patches: Mutex<Vec<Option<String>>>,
    buffer_multipliers: Mutex<Vec<u8>>,
    stops: Mutex<usize>,
}

impl PlayerHandle for FakePlayer {
    fn play_smf(&self, smf: Vec<u8>) -> Result<()> {
        self.plays.lock().unwrap().push(smf);
        Ok(())
    }

    fn play_mml(&self, mml: String) -> Result<()> {
        self.mml_plays.lock().unwrap().push(mml);
        Ok(())
    }

    fn send_midi(&self, messages: Vec<[u8; 3]>, patch: Option<String>) -> Result<()> {
        self.midi_batches.lock().unwrap().push(messages);
        self.midi_patches.lock().unwrap().push(patch);
        Ok(())
    }

    fn prepare_live_patch(&self, patch: Option<String>) -> Result<()> {
        self.prepared_live_patches.lock().unwrap().push(patch);
        Ok(())
    }

    fn set_live_buffer_multiplier(&self, multiplier: u8) -> Result<()> {
        self.buffer_multipliers.lock().unwrap().push(multiplier);
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        *self.stops.lock().unwrap() += 1;
        Ok(())
    }
}

#[test]
fn read_request_accepts_health_get_without_content_length() {
    let raw = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    let mut cursor = std::io::Cursor::new(raw);

    let request = read_request(&mut cursor, MAX_BODY_BYTES).unwrap();

    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/health");
    assert!(request.body.is_empty());
}

#[test]
fn read_request_empty_connection_is_quiet() {
    let raw = b"";
    let mut cursor = std::io::Cursor::new(raw);

    let error = read_request(&mut cursor, MAX_BODY_BYTES).unwrap_err();

    assert!(matches!(error.status, StatusCode::BadRequest));
    assert!(!error.respond);
}

#[test]
fn read_request_preserves_binary_play_body() {
    let raw = b"POST /play HTTP/1.1\r\nContent-Type: audio/midi\r\nContent-Length: 4\r\n\r\n\x00\x01\x02\xff";
    let mut cursor = std::io::Cursor::new(raw);

    let request = read_request(&mut cursor, MAX_BODY_BYTES).unwrap();

    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/play");
    assert_eq!(request.body, vec![0, 1, 2, 255]);
}

#[test]
fn read_request_rejects_body_over_limit() {
    let raw = b"POST /play HTTP/1.1\r\nContent-Type: audio/midi\r\nContent-Length: 5\r\n\r\n12345";
    let mut cursor = std::io::Cursor::new(raw);

    let error = read_request(&mut cursor, 4).unwrap_err();

    assert!(matches!(error.status, StatusCode::PayloadTooLarge));
}

#[test]
fn content_type_accepts_midi_binary_types() {
    assert!(content_type_is_midi("audio/midi"));
    assert!(content_type_is_midi("audio/x-midi; charset=binary"));
    assert!(content_type_is_midi("application/octet-stream"));
    assert!(content_type_is_midi("Audio/Midi"));
    assert!(!content_type_is_midi("text/plain"));
}

#[test]
fn run_realtime_play_server_handles_health() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(player, |addr| {
        send_raw_request(addr, "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
    });

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with("\r\n\r\nok"));
}

#[test]
fn run_realtime_play_server_dispatches_play() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request_bytes(
            addr,
            b"POST /play HTTP/1.1\r\nContent-Type: audio/midi\r\nContent-Length: 4\r\n\r\n\x00\x01\x02\xff",
        )
    });

    assert!(response.starts_with("HTTP/1.1 202 Accepted"));
    assert_eq!(
        player.plays.lock().unwrap().as_slice(),
        &[vec![0, 1, 2, 255]]
    );
}

#[test]
fn run_realtime_play_server_rejects_text_plain_play() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request(
            addr,
            "POST /play HTTP/1.1\r\nContent-Type: text/plain\r\nContent-Length: 3\r\n\r\nabc",
        )
    });

    assert!(response.starts_with("HTTP/1.1 415 Unsupported Media Type"));
    assert!(player.plays.lock().unwrap().is_empty());
}

#[test]
fn run_realtime_play_server_dispatches_play_mml() {
    let player = Arc::new(FakePlayer::default());
    let mml = "{\"Surge XT patch\": \"Keys/DX EP.fxp\"}cde";
    let request = format!(
        "POST /play-mml HTTP/1.1\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        mml.len(),
        mml
    );
    let response =
        run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

    assert!(response.starts_with("HTTP/1.1 202 Accepted"));
    assert_eq!(player.mml_plays.lock().unwrap().as_slice(), &[mml]);
    assert!(player.plays.lock().unwrap().is_empty());
}

#[test]
fn run_realtime_play_server_rejects_midi_content_type_for_play_mml() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request(
            addr,
            "POST /play-mml HTTP/1.1\r\nContent-Type: audio/midi\r\nContent-Length: 3\r\n\r\ncde",
        )
    });

    assert!(response.starts_with("HTTP/1.1 415 Unsupported Media Type"));
    assert!(player.mml_plays.lock().unwrap().is_empty());
}

#[test]
fn run_realtime_play_server_rejects_invalid_utf8_play_mml() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request_bytes(
            addr,
            b"POST /play-mml HTTP/1.1\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\n\xff\xfe",
        )
    });

    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(player.mml_plays.lock().unwrap().is_empty());
}

#[test]
fn run_realtime_play_server_rejects_get_play_mml() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(player, |addr| {
        send_raw_request(addr, "GET /play-mml HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
    });

    assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed"));
}

#[test]
fn content_type_accepts_text_plain_for_play_mml() {
    assert!(content_type_is_text("text/plain"));
    assert!(content_type_is_text("Text/Plain; charset=utf-8"));
    assert!(!content_type_is_text("audio/midi"));
    assert!(!content_type_is_text("application/octet-stream"));
}

#[test]
fn run_realtime_play_server_dispatches_polyphonic_midi_batch_in_order() {
    let player = Arc::new(FakePlayer::default());
    let body = r#"{"messages":[[144,60,100],[144,64,100],[144,67,100]]}"#;
    let request = format!(
        "POST /midi HTTP/1.1\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let response =
        run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

    assert!(response.starts_with("HTTP/1.1 202 Accepted"));
    assert_eq!(
        player.midi_batches.lock().unwrap().as_slice(),
        &[vec![[144, 60, 100], [144, 64, 100], [144, 67, 100]]]
    );
    assert_eq!(player.midi_patches.lock().unwrap().as_slice(), &[None]);
}

#[test]
fn run_realtime_play_server_dispatches_midi_patch() {
    let player = Arc::new(FakePlayer::default());
    let body = r#"{"messages":[[144,60,100]],"patch":"patches_factory/Keys/Piano.fxp"}"#;
    let request = format!(
        "POST /midi HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let response =
        run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

    assert!(response.starts_with("HTTP/1.1 202 Accepted"));
    assert_eq!(
        player.midi_patches.lock().unwrap().as_slice(),
        &[Some("patches_factory/Keys/Piano.fxp".to_string())]
    );
}

#[test]
fn run_realtime_play_server_rejects_empty_midi_batch() {
    let player = Arc::new(FakePlayer::default());
    let body = r#"{"messages":[]}"#;
    let request = format!(
        "POST /midi HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let response =
        run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(player.midi_batches.lock().unwrap().is_empty());
}

#[test]
fn run_realtime_play_server_rejects_invalid_midi_bytes() {
    for body in [
        r#"{"messages":[[144,128,0]]}"#,
        r#"{"messages":[[127,60,100]]}"#,
        r#"{"messages":[[144,60]]}"#,
    ] {
        let player = Arc::new(FakePlayer::default());
        let request = format!(
            "POST /midi HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let response =
            run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(player.midi_batches.lock().unwrap().is_empty());
    }
}

#[test]
fn run_realtime_play_server_requires_json_for_midi() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request(
            addr,
            "POST /midi HTTP/1.1\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\n{}",
        )
    });

    assert!(response.starts_with("HTTP/1.1 415 Unsupported Media Type"));
    assert!(player.midi_batches.lock().unwrap().is_empty());
}

#[test]
fn run_realtime_play_server_requires_content_length_for_midi() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request(
            addr,
            "POST /midi HTTP/1.1\r\nContent-Type: application/json\r\n\r\n",
        )
    });

    assert!(response.starts_with("HTTP/1.1 411 Length Required"));
    assert!(player.midi_batches.lock().unwrap().is_empty());
}

#[test]
fn run_realtime_play_server_rejects_get_midi() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(player, |addr| {
        send_raw_request(addr, "GET /midi HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
    });

    assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed"));
}

#[test]
fn run_realtime_play_server_waits_for_live_patch_preparation() {
    let player = Arc::new(FakePlayer::default());
    let body = r#"{"patch":"patches_factory/Keys/Piano.fxp"}"#;
    let request = format!(
        "POST /live-patch HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let response =
        run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

    assert!(response.starts_with("HTTP/1.1 204 No Content"));
    assert_eq!(
        player.prepared_live_patches.lock().unwrap().as_slice(),
        &[Some("patches_factory/Keys/Piano.fxp".to_string())]
    );
}

#[test]
fn run_realtime_play_server_prepares_init_patch_from_null() {
    let player = Arc::new(FakePlayer::default());
    let body = r#"{"patch":null}"#;
    let request = format!(
        "POST /live-patch HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let response =
        run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

    assert!(response.starts_with("HTTP/1.1 204 No Content"));
    assert_eq!(
        player.prepared_live_patches.lock().unwrap().as_slice(),
        &[None]
    );
}

#[test]
fn content_type_accepts_application_json_for_midi() {
    assert!(content_type_is_json("application/json"));
    assert!(content_type_is_json("Application/Json; charset=utf-8"));
    assert!(!content_type_is_json("text/plain"));
}

#[test]
fn run_realtime_play_server_sets_live_buffer_multiplier() {
    let player = Arc::new(FakePlayer::default());
    let body = r#"{"multiplier":8}"#;
    let request = format!(
        "POST /live-buffer HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let response =
        run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

    assert!(response.starts_with("HTTP/1.1 202 Accepted"));
    assert_eq!(player.buffer_multipliers.lock().unwrap().as_slice(), &[8]);
}

#[test]
fn run_realtime_play_server_rejects_invalid_live_buffer_multiplier() {
    let player = Arc::new(FakePlayer::default());
    let body = r#"{"multiplier":3}"#;
    let request = format!(
        "POST /live-buffer HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let response =
        run_one_request_server(Arc::clone(&player), |addr| send_raw_request(addr, &request));

    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(player.buffer_multipliers.lock().unwrap().is_empty());
}

#[test]
fn run_realtime_play_server_dispatches_stop() {
    let player = Arc::new(FakePlayer::default());
    let response = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request(addr, "POST /stop HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
    });

    assert!(response.starts_with("HTTP/1.1 204 No Content"));
    assert_eq!(*player.stops.lock().unwrap(), 1);
}

#[test]
fn write_empty_response_uses_zero_content_length() {
    let mut response = Vec::new();

    write_empty_response(&mut response, StatusCode::NoContent).unwrap();

    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 204 No Content\r\n"));
    assert!(response.contains("Content-Length: 0\r\n"));
    assert!(response.ends_with("\r\n\r\n"));
}

fn run_one_request_server(
    player: Arc<FakePlayer>,
    send: impl FnOnce(SocketAddr) -> String,
) -> String {
    let listener =
        TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind test listener");
    let addr = listener.local_addr().expect("read test listener address");
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = Arc::clone(&shutdown);
    let server_player: Arc<dyn PlayerHandle> = player;
    let server = std::thread::spawn(move || {
        run_realtime_play_server_on_listener(listener, server_shutdown, server_player)
    });

    let response = send(addr);
    shutdown.store(true, Ordering::SeqCst);
    server.join().expect("join test server").unwrap();
    response
}

fn send_raw_request(addr: SocketAddr, request: &str) -> String {
    send_raw_request_bytes(addr, request.as_bytes())
}

fn send_raw_request_bytes(addr: SocketAddr, request: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    stream.write_all(request).expect("write request bytes");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}
