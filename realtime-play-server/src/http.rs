use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crate::fast_ipc::spawn_fast_midi_server;
use crate::player::PlayerHandle;
use anyhow::{Context as _, Result};

mod response;
mod routes;

use response::*;
use routes::*;

const WORKERS: usize = 4;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const QUEUE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn run_realtime_play_server(
    port: u16,
    shutdown: Arc<AtomicBool>,
    player: Arc<dyn PlayerHandle>,
) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let bind_started = Instant::now();
    let listener = TcpListener::bind(addr)
        .with_context(|| format!("failed to bind realtime-play-server to {addr}"))?;
    crate::timing::log_phase("listen", bind_started.elapsed());
    run_realtime_play_server_on_listener(listener, shutdown, player)
}

fn run_realtime_play_server_on_listener(
    listener: TcpListener,
    shutdown: Arc<AtomicBool>,
    player: Arc<dyn PlayerHandle>,
) -> Result<()> {
    let addr = listener
        .local_addr()
        .context("failed to read realtime-play-server local address")?;
    listener
        .set_nonblocking(true)
        .context("failed to set listener nonblocking")?;
    let (connections_tx, connections_rx) = sync_channel(WORKERS);
    let mut worker_handles = spawn_play_workers(connections_rx, Arc::clone(&player))?;
    let fast_midi_handle =
        spawn_fast_midi_server(addr.port(), Arc::clone(&shutdown), Arc::clone(&player))?;
    eprintln!("clap-mml-realtime-play-server listening on http://{addr}");

    let accept_result = accept_connections(listener, &connections_tx, &shutdown);
    drop(connections_tx);
    join_play_workers(&mut worker_handles)?;
    if fast_midi_handle.is_some_and(|handle| handle.join().is_err()) {
        anyhow::bail!("shared-memory MIDI server panicked");
    }
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
                anyhow::bail!("realtime play worker pool stopped");
            }
        }
    }
}

fn spawn_play_workers(
    connections_rx: Receiver<TcpStream>,
    player: Arc<dyn PlayerHandle>,
) -> Result<Vec<JoinHandle<()>>> {
    let connections_rx = Arc::new(std::sync::Mutex::new(connections_rx));
    let mut handles = Vec::with_capacity(WORKERS);
    for worker_id in 0..WORKERS {
        let connections_rx = Arc::clone(&connections_rx);
        let player = Arc::clone(&player);
        let handle = std::thread::Builder::new()
            .name(format!("realtime-play-server-worker-{worker_id}"))
            .spawn(move || {
                run_play_worker(worker_id, connections_rx, player);
            })
            .context("failed to spawn realtime play HTTP worker")?;
        handles.push(handle);
    }
    Ok(handles)
}

fn run_play_worker(
    worker_id: usize,
    connections_rx: Arc<std::sync::Mutex<Receiver<TcpStream>>>,
    player: Arc<dyn PlayerHandle>,
) {
    loop {
        let stream = match connections_rx.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(mut stream) = stream else {
            return;
        };
        if let Err(error) = handle_connection(&mut stream, player.as_ref()) {
            eprintln!("worker {worker_id} request handling failed: {error:#}");
        }
    }
}

fn join_play_workers(worker_handles: &mut Vec<JoinHandle<()>>) -> Result<()> {
    for handle in worker_handles.drain(..) {
        if handle.join().is_err() {
            anyhow::bail!("realtime play HTTP worker panicked");
        }
    }
    Ok(())
}

fn handle_connection(stream: &mut TcpStream, player: &dyn PlayerHandle) -> Result<()> {
    stream
        .set_read_timeout(Some(REQUEST_READ_TIMEOUT))
        .context("failed to set request read timeout")?;

    let request = match read_request(stream, MAX_BODY_BYTES) {
        Ok(request) => request,
        Err(error) => {
            if error.respond {
                write_text_response(stream, error.status, &error.message)?;
            }
            return Ok(());
        }
    };

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => write_text_response(stream, StatusCode::Ok, "ok")?,
        ("POST", "/play") => handle_play_request(stream, player, request)?,
        ("POST", "/play-mml") => handle_play_mml_request(stream, player, request)?,
        ("POST", "/stop") => handle_stop_request(stream, player)?,
        (_, "/health" | "/play" | "/play-mml" | "/stop") => {
            write_text_response(stream, StatusCode::MethodNotAllowed, "method not allowed")?
        }
        _ => write_text_response(stream, StatusCode::NotFound, "not found")?,
    }

    Ok(())
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
    respond: bool,
}

impl RequestError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            respond: true,
        }
    }

    fn quiet(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            respond: false,
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
            if buffer.is_empty() {
                return Err(RequestError::quiet(
                    StatusCode::BadRequest,
                    "request ended before headers were complete",
                ));
            }
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
        .unwrap_or(0);
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

#[cfg(test)]
mod tests;
