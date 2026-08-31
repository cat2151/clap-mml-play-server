//! 受信ループが standby load 中も詰まらないことを、実共有メモリ越しに固定する。
//!
//! `FakePlayer` はプラグインを一切持たず、先読みの「完了」をテストが任意の時点で
//! 解放できる門にしてある。sleep の長さで順序を推測しない。
//!
//! **旧実装（受信ループがロード完了まで待つ形）ではここは通らない。**
//! `begin_standby_patch` の受付応答自体がロード完了まで返らないので、門を開けない
//! 限り 30 秒の応答 timeout で落ちる。

use super::*;
use cmrt_core::VoicingReport;
use cmrt_realtime_ipc::{
    FastIpcError, FastMidiClient, FastMidiEvent, LimiterMeter, LiveTempoChange, LiveTimelineConfig,
    TimelineMidiEvent, TimingMetrics,
};
use std::sync::{mpsc::SyncSender, Condvar, Mutex};

use crate::player::{standby_completion_channel, StandbyLoadResult};

/// realtime-ipc crate 側のテストと port が衝突しないよう、別の帯を使う。
/// あちらは 30_000..50_000。
fn test_port(offset: u16) -> u16 {
    20_000 + ((std::process::id() as u16).wrapping_add(offset) % 9_000)
}

const TIMELINE_ID: u64 = 4242;

#[derive(Default)]
struct FakePlayerState {
    /// 受け付けた先読みの完了を送る側。テストが好きな時点で解放する。
    standby_gates: Vec<SyncSender<StandbyLoadResult>>,
    standby_requests: Vec<u8>,
    timeline_batches: usize,
}

#[derive(Default)]
struct FakePlayer {
    state: Mutex<FakePlayerState>,
    changed: Condvar,
}

impl FakePlayer {
    /// `count` 件目の timeline batch が player へ届くまで待つ。届かなければ panic。
    fn wait_for_timeline_batches(&self, count: usize) {
        let deadline = Duration::from_secs(10);
        let mut state = self.state.lock().unwrap();
        while state.timeline_batches < count {
            let (next, timeout) = self.changed.wait_timeout(state, deadline).unwrap();
            state = next;
            assert!(
                !timeout.timed_out(),
                "timeline midi did not reach the player while a standby load was pending \
                 (batches={}, wanted={count})",
                state.timeline_batches
            );
        }
    }

    /// 先読みが受け付けられるまで待つ。
    fn wait_for_standby_requests(&self, count: usize) {
        let deadline = Duration::from_secs(10);
        let mut state = self.state.lock().unwrap();
        while state.standby_requests.len() < count {
            let (next, timeout) = self.changed.wait_timeout(state, deadline).unwrap();
            state = next;
            assert!(!timeout.timed_out(), "standby request was never accepted");
        }
    }

    fn timeline_batches(&self) -> usize {
        self.state.lock().unwrap().timeline_batches
    }

    /// 止めていた先読みの完了を解放する。
    fn release_standby(&self, index: usize, result: StandbyLoadResult) {
        let state = self.state.lock().unwrap();
        state.standby_gates[index].send(result).unwrap();
    }
}

impl PlayerHandle for FakePlayer {
    fn play_smf(&self, _smf: Vec<u8>) -> Result<()> {
        Ok(())
    }

    fn play_mml(&self, _mml: String) -> Result<()> {
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
        let mut state = self.state.lock().unwrap();
        state.timeline_batches += 1;
        self.changed.notify_all();
        Ok(())
    }

    fn prepare_live_patch(&self, _instance_id: InstanceId, _patch: Option<String>) -> Result<()> {
        Ok(())
    }

    fn begin_standby_live_patch(
        &self,
        instance_id: InstanceId,
        _patch: Option<String>,
    ) -> Result<StandbyLoadTicket> {
        let (gate, ticket) = standby_completion_channel();
        let mut state = self.state.lock().unwrap();
        state.standby_gates.push(gate);
        state.standby_requests.push(instance_id);
        self.changed.notify_all();
        Ok(ticket)
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

/// テスト用に受信ループを 1 本立てる。`Drop` で必ず畳む。
struct Harness {
    player: Arc<FakePlayer>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    client: FastMidiClient,
}

impl Harness {
    fn start(port: u16) -> Self {
        let server = FastMidiServer::create(port).unwrap();
        let player = Arc::new(FakePlayer::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let loop_player: Arc<dyn PlayerHandle> = Arc::clone(&player) as Arc<dyn PlayerHandle>;
        let loop_shutdown = Arc::clone(&shutdown);
        let thread = std::thread::Builder::new()
            .name("fast-ipc-test".to_string())
            .spawn(move || run_fast_midi_server(server, loop_shutdown, loop_player))
            .unwrap();
        let client = FastMidiClient::connect(port).unwrap();
        Self {
            player,
            shutdown,
            thread: Some(thread),
            client,
        }
    }

    /// 受信ループを止めて join する。孤児スレッドを残さない。
    fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }

    fn send_timeline_note(&mut self, seconds: f64) {
        self.client
            .send_timeline_events(&[TimelineMidiEvent {
                timeline_id: TIMELINE_ID,
                instance_id: 0,
                timeline_seconds: seconds,
                message: [0x90, 60, 100],
            }])
            .unwrap();
    }

    /// 完了通知が来るまでポーリングする。`poll` は block しないので、ここで回す。
    fn wait_for_completion(
        &self,
        request_id: u32,
        watermark: u64,
    ) -> std::result::Result<(), FastIpcError> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = self.client.poll_standby_completion(request_id, watermark) {
                return result;
            }
            assert!(
                Instant::now() < deadline,
                "standby completion was never published for request {request_id}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 実障害そのもの。ロードが終わらない間も timeline MIDI が player へ届く。
#[test]
fn timeline_midi_reaches_the_player_while_a_standby_load_is_still_loading() {
    let mut harness = Harness::start(test_port(0));
    let watermark = harness.client.standby_watermark();
    // 旧実装ならここがロード完了待ちで返らない。門は開けていない。
    let request_id = harness
        .client
        .begin_standby_patch(7, Some("Keys/Piano.fxp"))
        .unwrap();
    harness.player.wait_for_standby_requests(1);
    assert!(
        harness
            .client
            .poll_standby_completion(request_id, watermark)
            .is_none(),
        "受付応答はロード完了を意味してはいけない"
    );

    for step in 0..4 {
        harness.send_timeline_note(f64::from(step) * 0.25);
    }
    harness.player.wait_for_timeline_batches(4);

    // ここまで完了通知は一度も出ていない。
    assert!(harness
        .client
        .poll_standby_completion(request_id, watermark)
        .is_none());

    harness.player.release_standby(0, Ok(()));
    assert_eq!(harness.wait_for_completion(request_id, watermark), Ok(()));
    assert_eq!(harness.player.timeline_batches(), 4);
}

/// ロード失敗も専用 slot 側で運ぶ。受付応答は成功のままでよい。
#[test]
fn a_failed_standby_load_is_reported_through_the_completion_slot() {
    let mut harness = Harness::start(test_port(1));
    let watermark = harness.client.standby_watermark();
    let request_id = harness.client.begin_standby_patch(7, None).unwrap();
    harness.player.wait_for_standby_requests(1);
    harness
        .player
        .release_standby(0, Err("patch file is missing".to_string()));
    assert_eq!(
        harness.wait_for_completion(request_id, watermark),
        Err(FastIpcError::RequestFailed("patch file is missing".into()))
    );
}

/// 2 件目は受付で断る。完了 slot が 1 件しかないので、上書きすると 1 件目の
/// 完了通知が消える。
#[test]
fn a_second_standby_request_is_rejected_while_the_first_is_still_loading() {
    let mut harness = Harness::start(test_port(2));
    let watermark = harness.client.standby_watermark();
    let first = harness.client.begin_standby_patch(7, None).unwrap();
    harness.player.wait_for_standby_requests(1);

    let second = harness.client.begin_standby_patch(8, None);
    assert!(
        matches!(second, Err(FastIpcError::RequestFailed(ref message)) if message.contains("still in flight")),
        "2 件目は受付で断るはず: {second:?}"
    );
    // 断ったのだから player 側の要求は 1 件のまま。
    assert_eq!(
        harness.player.state.lock().unwrap().standby_requests.len(),
        1
    );

    harness.player.release_standby(0, Ok(()));
    assert_eq!(harness.wait_for_completion(first, watermark), Ok(()));
}

/// 受信ループが止まるときは、待たせている要求へ必ず error を返す。
/// 返さないとクライアントの進捗が永久に「ロード中」で残る。
#[test]
fn a_pending_standby_load_fails_when_the_receive_loop_stops() {
    let mut harness = Harness::start(test_port(3));
    let watermark = harness.client.standby_watermark();
    let request_id = harness.client.begin_standby_patch(7, None).unwrap();
    harness.player.wait_for_standby_requests(1);

    harness.stop();

    let completion = harness
        .client
        .poll_standby_completion(request_id, watermark);
    assert!(
        matches!(completion, Some(Err(FastIpcError::RequestFailed(ref message))) if message.contains("stopped")),
        "停止時は error 完了を publish するはず: {completion:?}"
    );
}
