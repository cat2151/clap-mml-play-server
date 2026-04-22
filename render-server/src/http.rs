use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Context as _, Result};
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const QUEUE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn run_render_server<R, MakeRender>(
    port: u16,
    workers: usize,
    shutdown: Arc<AtomicBool>,
    make_render: MakeRender,
) -> Result<()>
where
    R: FnMut(&str) -> Result<Vec<u8>> + Send + 'static,
    MakeRender: Fn() -> Result<R>,
{
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr)
        .with_context(|| format!("failed to bind render-server to {addr}"))?;
    run_render_server_on_listener(listener, workers, shutdown, make_render)
}

fn run_render_server_on_listener<R, MakeRender>(
    listener: TcpListener,
    workers: usize,
    shutdown: Arc<AtomicBool>,
    make_render: MakeRender,
) -> Result<()>
where
    R: FnMut(&str) -> Result<Vec<u8>> + Send + 'static,
    MakeRender: Fn() -> Result<R>,
{
    let addr = listener
        .local_addr()
        .context("failed to read render-server local address")?;
    listener
        .set_nonblocking(true)
        .context("failed to set listener nonblocking")?;
    let workers = workers.max(1);
    let (connections_tx, connections_rx) = sync_channel(workers);
    let mut worker_handles = spawn_render_workers(workers, connections_rx, make_render)?;
    eprintln!("clap-mml-render-server listening on http://{addr} with {workers} workers");

    let accept_result = accept_connections(listener, &connections_tx, &shutdown);
    drop(connections_tx);
    join_render_workers(&mut worker_handles)?;
    accept_result
}

fn accept_connections(
    listener: TcpListener,
    connections_tx: &SyncSender<TcpStream>,
    shutdown: &AtomicBool,
) -> Result<()> {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _peer)) => enqueue_connection(connections_tx, stream, shutdown)?,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(error) => return Err(error).context("failed to accept HTTP connection"),
        }
    }

    Ok(())
}

fn enqueue_connection(
    connections_tx: &SyncSender<TcpStream>,
    mut stream: TcpStream,
    shutdown: &AtomicBool,
) -> Result<()> {
    loop {
        match connections_tx.try_send(stream) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(returned_stream)) => {
                stream = returned_stream;
                if shutdown.load(Ordering::SeqCst) {
                    return Ok(());
                }
                std::thread::sleep(QUEUE_POLL_INTERVAL);
            }
            Err(TrySendError::Disconnected(_)) => {
                anyhow::bail!("render worker pool stopped");
            }
        }
    }
}

fn spawn_render_workers<R, MakeRender>(
    workers: usize,
    connections_rx: Receiver<TcpStream>,
    make_render: MakeRender,
) -> Result<Vec<JoinHandle<()>>>
where
    R: FnMut(&str) -> Result<Vec<u8>> + Send + 'static,
    MakeRender: Fn() -> Result<R>,
{
    let connections_rx = Arc::new(std::sync::Mutex::new(connections_rx));
    let mut handles = Vec::with_capacity(workers);
    for worker_id in 0..workers {
        let mut render = make_render()
            .with_context(|| format!("failed to initialize render worker {worker_id}"))?;
        let connections_rx = Arc::clone(&connections_rx);
        let handle = std::thread::Builder::new()
            .name(format!("render-server-worker-{worker_id}"))
            .spawn(move || {
                run_render_worker(worker_id, connections_rx, &mut render);
            })
            .context("failed to spawn render worker")?;
        handles.push(handle);
    }
    Ok(handles)
}

fn run_render_worker<R>(
    worker_id: usize,
    connections_rx: Arc<std::sync::Mutex<Receiver<TcpStream>>>,
    render: &mut R,
) where
    R: FnMut(&str) -> Result<Vec<u8>>,
{
    loop {
        let stream = match connections_rx.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(mut stream) = stream else {
            return;
        };
        if let Err(error) = handle_connection(&mut stream, render) {
            eprintln!("worker {worker_id} request handling failed: {error:#}");
        }
    }
}

fn join_render_workers(worker_handles: &mut Vec<JoinHandle<()>>) -> Result<()> {
    for handle in worker_handles.drain(..) {
        if handle.join().is_err() {
            anyhow::bail!("render worker panicked");
        }
    }
    Ok(())
}

fn handle_connection(
    stream: &mut TcpStream,
    render: &mut impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<()> {
    stream
        .set_read_timeout(Some(REQUEST_READ_TIMEOUT))
        .context("failed to set request read timeout")?;

    let request = match read_request(stream, MAX_BODY_BYTES) {
        Ok(request) => request,
        Err(error) => {
            write_text_response(stream, error.status, &error.message)?;
            return Ok(());
        }
    };

    if request.method != "POST" {
        write_text_response(stream, StatusCode::MethodNotAllowed, "method not allowed")?;
        return Ok(());
    }
    if request.path != "/render" {
        write_text_response(stream, StatusCode::NotFound, "not found")?;
        return Ok(());
    }
    if !request
        .header("content-type")
        .is_some_and(content_type_is_text_plain)
    {
        write_text_response(
            stream,
            StatusCode::UnsupportedMediaType,
            "Content-Type must be text/plain; charset=utf-8",
        )?;
        return Ok(());
    }

    let mml = match String::from_utf8(request.body) {
        Ok(mml) => mml,
        Err(_) => {
            write_text_response(
                stream,
                StatusCode::BadRequest,
                "request body must be valid UTF-8",
            )?;
            return Ok(());
        }
    };

    match render(&mml) {
        Ok(wav) => write_binary_response(stream, StatusCode::Ok, "audio/wav", &wav)?,
        Err(error) => write_text_response(
            stream,
            StatusCode::InternalServerError,
            &format!("{error:#}"),
        )?,
    }
    Ok(())
}

fn content_type_is_text_plain(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/plain"))
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug)]
struct RequestError {
    status: StatusCode,
    message: String,
}

impl RequestError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

fn read_request(
    reader: &mut impl std::io::Read,
    max_body_bytes: usize,
) -> Result<HttpRequest, RequestError> {
    let mut buffer = Vec::new();
    let mut scratch = [0u8; 4096];
    let header_end = loop {
        if let Some(header_end) = find_header_end(&buffer) {
            break header_end;
        }
        if buffer.len() >= MAX_HEADER_BYTES {
            return Err(RequestError::new(
                StatusCode::RequestHeaderFieldsTooLarge,
                "request headers are too large",
            ));
        }
        let read = reader.read(&mut scratch).map_err(|error| {
            RequestError::new(
                StatusCode::BadRequest,
                format!("failed to read request: {error}"),
            )
        })?;
        if read == 0 {
            return Err(RequestError::new(
                StatusCode::BadRequest,
                "request ended before headers were complete",
            ));
        }
        buffer.extend_from_slice(&scratch[..read]);
    };

    let head = std::str::from_utf8(&buffer[..header_end - 4]).map_err(|_| {
        RequestError::new(
            StatusCode::BadRequest,
            "request headers must be valid UTF-8",
        )
    })?;
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| RequestError::new(StatusCode::BadRequest, "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| RequestError::new(StatusCode::BadRequest, "missing request method"))?;
    let path = parts
        .next()
        .ok_or_else(|| RequestError::new(StatusCode::BadRequest, "missing request path"))?;
    let _version = parts
        .next()
        .ok_or_else(|| RequestError::new(StatusCode::BadRequest, "missing HTTP version"))?;
    if parts.next().is_some() {
        return Err(RequestError::new(
            StatusCode::BadRequest,
            "malformed request line",
        ));
    }

    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(RequestError::new(
                StatusCode::BadRequest,
                "malformed request header",
            ));
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }

    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| {
            value
                .parse::<usize>()
                .map_err(|_| RequestError::new(StatusCode::BadRequest, "invalid Content-Length"))
        })
        .transpose()?
        .ok_or_else(|| RequestError::new(StatusCode::LengthRequired, "Content-Length required"))?;
    if content_length > max_body_bytes {
        return Err(RequestError::new(
            StatusCode::PayloadTooLarge,
            format!("request body is too large; limit is {max_body_bytes} bytes"),
        ));
    }

    let mut body = buffer[header_end..].to_vec();
    if body.len() > content_length {
        body.truncate(content_length);
    }
    while body.len() < content_length {
        let read = reader.read(&mut scratch).map_err(|error| {
            RequestError::new(
                StatusCode::BadRequest,
                format!("failed to read request body: {error}"),
            )
        })?;
        if read == 0 {
            return Err(RequestError::new(
                StatusCode::BadRequest,
                "request ended before body was complete",
            ));
        }
        let remaining = content_length - body.len();
        body.extend_from_slice(&scratch[..read.min(remaining)]);
    }

    Ok(HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        headers,
        body,
    })
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

#[derive(Clone, Copy, Debug)]
enum StatusCode {
    Ok,
    BadRequest,
    NotFound,
    MethodNotAllowed,
    LengthRequired,
    PayloadTooLarge,
    UnsupportedMediaType,
    RequestHeaderFieldsTooLarge,
    InternalServerError,
}

impl StatusCode {
    fn code(self) -> u16 {
        match self {
            Self::Ok => 200,
            Self::BadRequest => 400,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::LengthRequired => 411,
            Self::PayloadTooLarge => 413,
            Self::UnsupportedMediaType => 415,
            Self::RequestHeaderFieldsTooLarge => 431,
            Self::InternalServerError => 500,
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::BadRequest => "Bad Request",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::LengthRequired => "Length Required",
            Self::PayloadTooLarge => "Payload Too Large",
            Self::UnsupportedMediaType => "Unsupported Media Type",
            Self::RequestHeaderFieldsTooLarge => "Request Header Fields Too Large",
            Self::InternalServerError => "Internal Server Error",
        }
    }
}

fn write_text_response(
    stream: &mut impl std::io::Write,
    status: StatusCode,
    message: &str,
) -> Result<()> {
    write_binary_response(
        stream,
        status,
        "text/plain; charset=utf-8",
        message.as_bytes(),
    )
}

fn write_binary_response(
    stream: &mut impl std::io::Write,
    status: StatusCode,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status.code(),
        status.reason(),
        content_type,
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
