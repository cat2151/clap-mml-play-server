//! 共有メモリ IPC の受信ループ。
//!
//! # ここは絶対に塞いではいけない
//! このスレッドが 1 本で、`TimelineMidi` も `SetLiveTempo` も heartbeat も meter の
//! publish も全部ここを通る。1 コマンドの処理で待つと、その間クライアントが積んだ
//! timeline event は共有リングに溜まったまま player へ渡らない。
//!
//! 実際にそれで壊れたのが standby patch の先読みで、重い音色のロードが 3 秒近く
//! 掛かる間、受信ループがロード完了を待って止まっていた。演奏 bank の render は
//! 続いていた（`underrun_frames=0`）のに、次の note-off が届かず 16 分音符が
//! 全音符まで伸びた。protocol v10 でこれを 2 段階へ分けてある。
//!
//! - **受付応答**: 汎用 `FastMidiServer::complete_request`。要求を player へ
//!   積めたかどうかだけを返す。ロード完了ではない。
//! - **完了通知**: 専用 slot `FastMidiServer::publish_standby_completion`。
//!   bank worker のロードが終わってからサーバーが一方的に書く。
//!
//! 受信ループは受付票（[`StandbyLoadTicket`]）を状態として持ち、毎周回
//! 非 blocking に poll するだけ。

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use cmrt_realtime_ipc::{FastMidiCommand, FastMidiServer, InstanceId};

use crate::player::{PlayerHandle, StandbyLoadTicket};

const IPC_WAIT_TIMEOUT: Duration = Duration::from_millis(50);

#[cfg(windows)]
pub(crate) fn spawn_fast_midi_server(
    port: u16,
    shutdown: Arc<AtomicBool>,
    player: Arc<dyn PlayerHandle>,
) -> Result<Option<JoinHandle<()>>> {
    let server =
        FastMidiServer::create(port).context("failed to create shared-memory MIDI server")?;
    let handle = std::thread::Builder::new()
        .name("realtime-play-server-fast-midi".to_string())
        .spawn(move || run_fast_midi_server(server, shutdown, player))
        .context("failed to spawn shared-memory MIDI server")?;
    Ok(Some(handle))
}

#[cfg(not(windows))]
pub(crate) fn spawn_fast_midi_server(
    _port: u16,
    _shutdown: Arc<AtomicBool>,
    _player: Arc<dyn PlayerHandle>,
) -> Result<Option<JoinHandle<()>>> {
    Ok(None)
}

/// 受け付け済みで、まだロードが終わっていない先読み。
///
/// 完了 slot は 1 件しか無く、Grid Sequencer は「1 件ずつ順番に先読みする」契約な
/// ので、ここも同時に 1 件だけ持つ。2 件目は受付で断る。上書きすると 1 件目の
/// 完了通知が消え、そのクライアントが永久に待つため。
struct PendingStandbyLoad {
    request_id: u32,
    instance_id: InstanceId,
    ticket: StandbyLoadTicket,
    started: Instant,
}

fn run_fast_midi_server(
    mut server: FastMidiServer,
    shutdown: Arc<AtomicBool>,
    player: Arc<dyn PlayerHandle>,
) {
    let mut standby: Option<PendingStandbyLoad> = None;
    while !shutdown.load(Ordering::SeqCst) {
        // 毎周回ここで拾う。コマンドが続いている間は `recv_timeout` が即座に戻るので
        // 実質コマンドごと、暇なら最悪 `IPC_WAIT_TIMEOUT` 遅れて通知が出る。
        // クライアントは非 blocking にポーリングする前提なのでこれで足りる。
        publish_ready_standby(&server, &mut standby);
        server.publish_limiter_meter(player.limiter_meter());
        server.publish_underrun_frames(player.underrun_frames());
        server.publish_auto_gain_db(&player.auto_gain_db());
        server.publish_timing_metrics(player.timing_metrics());
        match server.recv_timeout(IPC_WAIT_TIMEOUT) {
            Ok(Some(command)) => dispatch(command, player.as_ref(), &server, &mut standby),
            Ok(None) => {}
            Err(error) => eprintln!("shared-memory MIDI receive failed: {error}"),
        }
    }
    // 停止するときも、待たせている要求には必ず結果を返す。返さないとクライアントの
    // 進捗表示が「ロード中」のまま永久に畳まれない。
    fail_pending_standby(
        &server,
        standby.take(),
        "shared-memory MIDI server stopped before the standby patch load finished",
    );
}

/// 受け取ったコマンドを 1 行で出す。
///
/// クライアント側のログは「送った」までしか書けない。音が止まらないときに
/// 「そもそも届いていない」のか「届いたが効いていない」のかを切り分けるには、
/// 受け口のここで届いた順に記録するしかない。
fn log_received(command: &FastMidiCommand) {
    match command {
        FastMidiCommand::Midi { events } => {
            let summary = events
                .iter()
                .map(|event| {
                    format!(
                        "i{}:{:02x}:{}:{}",
                        event.instance_id, event.message[0], event.message[1], event.message[2]
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            eprintln!(
                "cmrt-ipc-recv: kind=midi count={} [{summary}]",
                events.len()
            );
        }
        FastMidiCommand::StopAll => eprintln!("cmrt-ipc-recv: kind=stop-all"),
        FastMidiCommand::Stop { instance_id } => {
            eprintln!("cmrt-ipc-recv: kind=stop instance={instance_id}")
        }
        FastMidiCommand::BeginLiveTimeline(config) => {
            eprintln!(
                "cmrt-ipc-recv: kind=begin-timeline id={}",
                config.timeline_id
            )
        }
        FastMidiCommand::TimelineMidi { events } => {
            eprintln!("cmrt-ipc-recv: kind=timeline-midi count={}", events.len())
        }
        FastMidiCommand::PrepareStandbyPatch {
            request_id,
            instance_id,
            patch,
        } => {
            eprintln!(
                "cmrt-ipc-recv: kind=prepare-standby-patch request={request_id} \
                 instance={instance_id} patch={patch:?}"
            )
        }
        _ => {}
    }
}

/// 同期応答を返して、その結果を dispatch の戻り値へ畳む。
///
/// **standby ではこれは「受付応答」であってロード完了ではない。** 完了は
/// [`publish_standby_completion`] が専用 slot へ書く。
///
/// 応答を返し損ねるとクライアントは 30 秒の timeout まで待たされる。patch 系の
/// コマンドが増えるたびにこの手順を写経しないよう、1 か所にまとめてある。
fn respond(server: &FastMidiServer, request_id: u32, response: Result<Vec<u8>>) -> Result<()> {
    let completion = match &response {
        Ok(payload) => server.complete_request(request_id, Ok(payload)),
        Err(error) => {
            let message = format!("{error:#}");
            server.complete_request(request_id, Err(&message))
        }
    };
    if let Err(error) = completion {
        eprintln!("shared-memory response failed: {error}");
    }
    response.map(|_| ())
}

fn dispatch(
    command: FastMidiCommand,
    player: &dyn PlayerHandle,
    server: &FastMidiServer,
    standby: &mut Option<PendingStandbyLoad>,
) {
    log_received(&command);
    let result = match command {
        FastMidiCommand::Midi { events } => player.send_midi(events),
        FastMidiCommand::BeginLiveTimeline(config) => player.begin_live_timeline(config),
        FastMidiCommand::SetLiveTempo(change) => player.set_live_tempo(change),
        FastMidiCommand::TimelineMidi { events } => player.send_timeline_midi(events),
        FastMidiCommand::PreparePatch {
            request_id,
            instance_id,
            patch,
            probe,
        } => {
            let response = if probe {
                player
                    .prepare_live_patch_with_voicing(instance_id, patch)
                    .and_then(|report| serde_json::to_vec(&report).map_err(Into::into))
            } else {
                player
                    .prepare_live_patch(instance_id, patch)
                    .map(|()| Vec::new())
            };
            respond(server, request_id, response)
        }
        FastMidiCommand::PrepareStandbyPatch {
            request_id,
            instance_id,
            patch,
        } => accept_standby(server, player, standby, request_id, instance_id, patch),
        FastMidiCommand::SetBufferMultiplier { multiplier } => {
            player.set_live_buffer_multiplier(multiplier)
        }
        FastMidiCommand::SetInstanceGain {
            instance_id,
            gain_milli,
        } => player.set_live_instance_gain(instance_id, gain_milli as f32 / 1000.0),
        FastMidiCommand::SetAutoGain { enabled } => player.set_live_auto_gain_enabled(enabled),
        FastMidiCommand::Stop { instance_id } => player.stop_instance(instance_id),
        FastMidiCommand::StopAll => player.stop(),
    };
    if let Err(error) = result {
        eprintln!("shared-memory MIDI command failed: {error:#}");
    }
}

/// 先読み要求を受け付ける。**ここでロードを待たない。**
///
/// 汎用応答で返すのは「player へ積めたか」だけ。積めたら受付票を持ち帰り、
/// 受信ループの毎周回でポーリングする。
fn accept_standby(
    server: &FastMidiServer,
    player: &dyn PlayerHandle,
    standby: &mut Option<PendingStandbyLoad>,
    request_id: u32,
    instance_id: InstanceId,
    patch: Option<String>,
) -> Result<()> {
    if let Some(active) = standby.as_ref() {
        eprintln!(
            "cmrt-standby-patch: request={request_id} instance={instance_id} \
             event=rejected detail=busy active_request={}",
            active.request_id
        );
        return respond(
            server,
            request_id,
            Err(anyhow::anyhow!(
                "standby patch load {} is still in flight",
                active.request_id
            )),
        );
    }
    match player.begin_standby_live_patch(instance_id, patch) {
        Ok(ticket) => {
            eprintln!(
                "cmrt-standby-patch: request={request_id} instance={instance_id} event=accepted"
            );
            *standby = Some(PendingStandbyLoad {
                request_id,
                instance_id,
                ticket,
                started: Instant::now(),
            });
            respond(server, request_id, Ok(Vec::new()))
        }
        Err(error) => {
            eprintln!(
                "cmrt-standby-patch: request={request_id} instance={instance_id} \
                 event=rejected detail={error:#}"
            );
            respond(server, request_id, Err(error))
        }
    }
}

/// ロードが終わっていれば専用 slot へ完了通知を publish する。
///
/// **終わっていなければ何もせずに戻る。** ここで待たないことがこのモジュールの
/// 目的そのもの。
fn publish_ready_standby(server: &FastMidiServer, standby: &mut Option<PendingStandbyLoad>) {
    let Some(load) = standby.as_ref() else {
        return;
    };
    let Some(result) = load.ticket.poll() else {
        return;
    };
    let load = standby.take().expect("直前に Some を確かめている");
    publish_standby_completion(server, &load, result, "completed");
}

/// 受信ループを畳むときに、待たせている要求へ error を返す。
fn fail_pending_standby(
    server: &FastMidiServer,
    standby: Option<PendingStandbyLoad>,
    reason: &str,
) {
    let Some(load) = standby else {
        return;
    };
    publish_standby_completion(server, &load, Err(anyhow::anyhow!("{reason}")), "abandoned");
}

/// 完了通知を専用 slot へ書き、同じ内容を 1 行のログにも残す。
///
/// `request=` を必ず含めるのは、受付（`event=accepted`）と完了（`event=completed`）
/// を後からログだけで突き合わせられるようにするため。
fn publish_standby_completion(
    server: &FastMidiServer,
    load: &PendingStandbyLoad,
    result: Result<()>,
    event: &str,
) {
    let message = result.err().map(|error| format!("{error:#}"));
    eprintln!(
        "cmrt-standby-patch: request={} instance={} event={event} elapsed_ms={} result={}{}",
        load.request_id,
        load.instance_id,
        load.started.elapsed().as_millis(),
        if message.is_some() { "error" } else { "ok" },
        message
            .as_deref()
            .map(|detail| format!(" detail={detail}"))
            .unwrap_or_default(),
    );
    let published = match message.as_deref() {
        None => server.publish_standby_completion(load.request_id, Ok(())),
        Some(message) => server.publish_standby_completion(load.request_id, Err(message)),
    };
    if let Err(error) = published {
        eprintln!("shared-memory standby completion failed: {error}");
    }
}

// 共有メモリを実際に張るので Windows 限定。
#[cfg(all(test, windows))]
mod tests;
