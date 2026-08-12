use super::*;
use cmrt_core::VoicingReport;
use cmrt_realtime_ipc::{
    FastMidiEvent, InstanceId, LimiterMeter, LiveTempoChange, LiveTimelineConfig,
    TimelineMidiEvent, TimingMetrics,
};
use std::{
    io::{Read as _, Write as _},
    sync::Mutex,
};

#[derive(Default)]
struct FakePlayer {
    plays: Mutex<Vec<Vec<u8>>>,
    mml_plays: Mutex<Vec<String>>,
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

    fn send_midi(&self, _events: Vec<FastMidiEvent>) -> Result<()> {
        Ok(())
    }

    fn begin_live_timeline(&self, _config: LiveTimelineConfig) -> Result<()> {
        Ok(())
    }

    fn set_live_tempo(&self, _change: LiveTempoChange) -> Result<()> {
        Ok(())
    }

    fn send_timeline_midi(&self, _events: Vec<TimelineMidiEvent>) -> Result<()> {
        Ok(())
    }

    fn prepare_live_patch(&self, _instance_id: InstanceId, _patch: Option<String>) -> Result<()> {
        Ok(())
    }

    fn prepare_live_patch_with_voicing(
        &self,
        _instance_id: InstanceId,
        _patch: Option<String>,
    ) -> Result<VoicingReport> {
        anyhow::bail!("not used")
    }

    fn set_live_buffer_multiplier(&self, _multiplier: u16) -> Result<()> {
        Ok(())
    }

    fn set_live_instance_gain(&self, _instance_id: InstanceId, _gain: f32) -> Result<()> {
        Ok(())
    }

    fn set_live_auto_gain_enabled(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn stop_instance(&self, _instance_id: InstanceId) -> Result<()> {
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        *self.stops.lock().unwrap() += 1;
        Ok(())
    }

    fn limiter_meter(&self) -> LimiterMeter {
        LimiterMeter::default()
    }

    fn underrun_frames(&self) -> u64 {
        0
    }

    fn timing_metrics(&self) -> TimingMetrics {
        TimingMetrics::default()
    }

    fn auto_gain_db(&self) -> Vec<f32> {
        Vec::new()
    }
}

#[test]
fn read_request_accepts_health_get_without_content_length() {
    let raw = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    let request = read_request(&mut std::io::Cursor::new(raw), MAX_BODY_BYTES).unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/health");
    assert!(request.body.is_empty());
}

#[test]
fn read_request_preserves_binary_body_and_rejects_oversize() {
    let raw = b"POST /play HTTP/1.1\r\nContent-Type: audio/midi\r\nContent-Length: 4\r\n\r\n\x00\x01\x02\xff";
    let request = read_request(&mut std::io::Cursor::new(raw), 4).unwrap();
    assert_eq!(request.body, vec![0, 1, 2, 255]);
    let error = read_request(&mut std::io::Cursor::new(raw), 3).unwrap_err();
    assert!(matches!(error.status, StatusCode::PayloadTooLarge));
}

#[test]
fn server_keeps_only_health_play_play_mml_and_stop_routes() {
    let player = Arc::new(FakePlayer::default());
    let health = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request(addr, "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
    });
    assert!(health.starts_with("HTTP/1.1 200 OK"));

    for removed in ["/midi", "/live-patch", "/live-patch-probe", "/live-buffer"] {
        let response = run_one_request_server(Arc::clone(&player), |addr| {
            send_raw_request(
                addr,
                &format!("POST {removed} HTTP/1.1\r\nContent-Length: 0\r\n\r\n"),
            )
        });
        assert!(response.starts_with("HTTP/1.1 404 Not Found"));
    }
}

#[test]
fn server_dispatches_play_play_mml_and_stop() {
    let player = Arc::new(FakePlayer::default());
    let play = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request_bytes(
            addr,
            b"POST /play HTTP/1.1\r\nContent-Type: audio/midi\r\nContent-Length: 4\r\n\r\n\x00\x01\x02\xff",
        )
    });
    assert!(play.starts_with("HTTP/1.1 202 Accepted"));
    assert_eq!(
        player.plays.lock().unwrap().as_slice(),
        &[vec![0, 1, 2, 255]]
    );

    let play_mml = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request(
            addr,
            "POST /play-mml HTTP/1.1\r\nContent-Type: text/plain\r\nContent-Length: 3\r\n\r\ncde",
        )
    });
    assert!(play_mml.starts_with("HTTP/1.1 202 Accepted"));
    assert_eq!(player.mml_plays.lock().unwrap().as_slice(), &["cde"]);

    let stop = run_one_request_server(Arc::clone(&player), |addr| {
        send_raw_request(addr, "POST /stop HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
    });
    assert!(stop.starts_with("HTTP/1.1 204 No Content"));
    assert_eq!(*player.stops.lock().unwrap(), 1);
}

#[test]
fn content_types_are_case_insensitive_and_allow_parameters() {
    assert!(content_type_is_midi("Audio/Midi"));
    assert!(content_type_is_midi("audio/x-midi; charset=binary"));
    assert!(content_type_is_text("Text/Plain; charset=utf-8"));
    assert!(!content_type_is_text("application/json"));
}

#[test]
fn write_empty_response_uses_zero_content_length() {
    let mut response = Vec::new();
    write_empty_response(&mut response, StatusCode::NoContent).unwrap();
    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 204 No Content\r\n"));
    assert!(response.contains("Content-Length: 0\r\n"));
}

fn run_one_request_server<F>(player: Arc<FakePlayer>, request: F) -> String
where
    F: FnOnce(SocketAddr) -> String,
{
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = Arc::clone(&shutdown);
    let server_player: Arc<dyn PlayerHandle> = player;
    let server = std::thread::spawn(move || {
        run_realtime_play_server_on_listener(listener, server_shutdown, server_player)
    });
    let response = request(addr);
    shutdown.store(true, Ordering::SeqCst);
    TcpStream::connect(addr).unwrap();
    server.join().unwrap().unwrap();
    response
}

fn send_raw_request(addr: SocketAddr, request: &str) -> String {
    send_raw_request_bytes(addr, request.as_bytes())
}

fn send_raw_request_bytes(addr: SocketAddr, request: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(request).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
