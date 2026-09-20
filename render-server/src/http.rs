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

use anyhow::{Context as _, Result};

mod connection_log;
mod content_type;
mod request;
mod response;

use connection_log::ConnectionLog;
use content_type::is_text_plain;
use request::read_request;
use response::{write_binary_response, StatusCode, TEXT_PLAIN_UTF8};

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
    connection_log::mark_listen_started();
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
        let log = ConnectionLog::accepted(worker_id);
        if let Err(error) = handle_connection(&mut stream, render, &log) {
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
    log: &ConnectionLog,
) -> Result<()> {
    stream
        .set_read_timeout(Some(REQUEST_READ_TIMEOUT))
        .context("failed to set request read timeout")?;

    let request = match read_request(stream, MAX_BODY_BYTES) {
        Ok(request) => request,
        Err(error) => {
            log.event(
                "request-rejected",
                format_args!(
                    "status={} reason=\"{}\"",
                    error.status.code(),
                    error.message
                ),
            );
            return write_logged_text_response(stream, log, error.status, &error.message);
        }
    };
    log.event(
        "request-read",
        format_args!(
            "method={} path={} body_bytes={}",
            request.method,
            request.path,
            request.body.len()
        ),
    );

    if request.method != "POST" {
        return write_logged_text_response(
            stream,
            log,
            StatusCode::MethodNotAllowed,
            "method not allowed",
        );
    }
    if request.path != "/render" {
        return write_logged_text_response(stream, log, StatusCode::NotFound, "not found");
    }
    if !request.header("content-type").is_some_and(is_text_plain) {
        return write_logged_text_response(
            stream,
            log,
            StatusCode::UnsupportedMediaType,
            "Content-Type must be text/plain; charset=utf-8",
        );
    }

    let mml = match String::from_utf8(request.body) {
        Ok(mml) => mml,
        Err(_) => {
            return write_logged_text_response(
                stream,
                log,
                StatusCode::BadRequest,
                "request body must be valid UTF-8",
            );
        }
    };

    log.event("render-start", format_args!(""));
    let render_started = Instant::now();
    let rendered = render(&mml);
    let render_ms = render_started.elapsed().as_millis();
    match rendered {
        Ok(wav) => {
            log.event(
                "render-end",
                format_args!("result=ok render_ms={render_ms} wav_bytes={}", wav.len()),
            );
            write_logged_response(stream, log, StatusCode::Ok, "audio/wav", &wav)
        }
        Err(error) => {
            log.event(
                "render-end",
                format_args!("result=error render_ms={render_ms} error=\"{error:#}\""),
            );
            write_logged_text_response(
                stream,
                log,
                StatusCode::InternalServerError,
                &format!("{error:#}"),
            )
        }
    }
}

fn write_logged_text_response(
    stream: &mut TcpStream,
    log: &ConnectionLog,
    status: StatusCode,
    message: &str,
) -> Result<()> {
    write_logged_response(stream, log, status, TEXT_PLAIN_UTF8, message.as_bytes())
}

/// 応答を書き、書けたか（相手がまだ居たか）を接続ログに残す。
fn write_logged_response(
    stream: &mut TcpStream,
    log: &ConnectionLog,
    status: StatusCode,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    match write_binary_response(stream, status, content_type, body) {
        Ok(()) => {
            log.event(
                "response-written",
                format_args!("status={} body_bytes={}", status.code(), body.len()),
            );
            Ok(())
        }
        Err(error) => {
            log.event(
                "response-write-failed",
                format_args!("status={} error=\"{error:#}\"", status.code()),
            );
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
