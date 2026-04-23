use super::*;
use std::io::Write as _;

#[test]
fn read_request_accepts_render_post() {
    let raw = b"POST /render HTTP/1.1\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 3\r\n\r\ncde";
    let mut cursor = std::io::Cursor::new(raw);

    let request = read_request(&mut cursor, MAX_BODY_BYTES).unwrap();

    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/render");
    assert_eq!(
        request.header("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(request.body, b"cde");
}

#[test]
fn read_request_rejects_body_over_limit() {
    let raw =
        b"POST /render HTTP/1.1\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\n12345";
    let mut cursor = std::io::Cursor::new(raw);

    let error = read_request(&mut cursor, 4).unwrap_err();

    assert!(matches!(error.status, StatusCode::PayloadTooLarge));
}

#[test]
fn content_type_accepts_charset_suffix() {
    assert!(content_type_is_text_plain("text/plain; charset=utf-8"));
    assert!(content_type_is_text_plain("Text/Plain"));
    assert!(!content_type_is_text_plain("application/json"));
}

#[test]
fn write_text_response_uses_plain_text_headers() {
    let mut response = Vec::new();

    write_text_response(&mut response, StatusCode::BadRequest, "invalid request").unwrap();

    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    assert!(response.contains("Content-Type: text/plain; charset=utf-8\r\n"));
    assert!(response.ends_with("\r\n\r\ninvalid request"));
}

#[test]
fn run_render_server_processes_worker_requests_concurrently() {
    let listener =
        TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind test listener");
    let addr = listener.local_addr().expect("read test listener address");
    let shutdown = Arc::new(AtomicBool::new(false));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));

    let server_shutdown = Arc::clone(&shutdown);
    let server_gate = Arc::clone(&gate);
    let make_render = move || {
        let started_tx = started_tx.clone();
        let gate = Arc::clone(&server_gate);
        Ok(move |_mml: &str| {
            started_tx.send(()).expect("record render start");
            let (lock, cvar) = &*gate;
            let mut released = lock.lock().expect("lock release gate");
            while !*released {
                released = cvar.wait(released).expect("wait release gate");
            }
            Ok(b"wav".to_vec())
        })
    };
    let server = std::thread::spawn(move || {
        run_render_server_on_listener(listener, 2, server_shutdown, make_render)
    });
    let client_a = std::thread::spawn(move || send_render_request(addr, "c"));
    let client_b = std::thread::spawn(move || send_render_request(addr, "d"));

    let first_started = started_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    let second_started = started_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    {
        let (lock, cvar) = &*gate;
        *lock.lock().expect("lock release gate") = true;
        cvar.notify_all();
    }

    let response_a = client_a.join().expect("join first client");
    let response_b = client_b.join().expect("join second client");
    shutdown.store(true, Ordering::SeqCst);
    server.join().expect("join test server").unwrap();

    assert!(
        first_started && second_started,
        "two render requests should start before either render is released"
    );
    assert!(response_a.starts_with("HTTP/1.1 200 OK"));
    assert!(response_b.starts_with("HTTP/1.1 200 OK"));
}

#[test]
fn run_render_server_reports_worker_initialization_error() {
    fn render_never_called(_: &str) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }

    let listener =
        TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind test listener");
    let shutdown = Arc::new(AtomicBool::new(false));
    let error = run_render_server_on_listener(listener, 1, shutdown, || {
        anyhow::bail!("init failed");
        #[allow(unreachable_code)]
        Ok(render_never_called as fn(&str) -> Result<Vec<u8>>)
    })
    .unwrap_err();

    assert!(error.to_string().contains("worker 0"));
}

fn send_render_request(addr: SocketAddr, body: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    write!(
        stream,
        "POST /render HTTP/1.1\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write request");
    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response).expect("read response");
    response
}
