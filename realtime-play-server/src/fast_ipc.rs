use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Context as _, Result};
use cmrt_realtime_ipc::{FastMidiCommand, FastMidiServer};

use crate::player::PlayerHandle;

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

fn run_fast_midi_server(
    mut server: FastMidiServer,
    shutdown: Arc<AtomicBool>,
    player: Arc<dyn PlayerHandle>,
) {
    while !shutdown.load(Ordering::SeqCst) {
        server.publish_limiter_meter(player.limiter_meter());
        server.publish_underrun_frames(player.underrun_frames());
        server.publish_auto_gain_db(&player.auto_gain_db());
        server.publish_timing_metrics(player.timing_metrics());
        match server.recv_timeout(IPC_WAIT_TIMEOUT) {
            Ok(Some(command)) => dispatch(command, player.as_ref(), &server),
            Ok(None) => {}
            Err(error) => eprintln!("shared-memory MIDI receive failed: {error}"),
        }
    }
}

fn dispatch(command: FastMidiCommand, player: &dyn PlayerHandle, server: &FastMidiServer) {
    let result = match command {
        FastMidiCommand::Midi { events } => player.send_midi(events),
        FastMidiCommand::BeginLiveTimeline(config) => player.begin_live_timeline(config),
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
