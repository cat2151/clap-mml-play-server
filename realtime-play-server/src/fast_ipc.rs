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
        match server.recv_timeout(IPC_WAIT_TIMEOUT) {
            Ok(Some(command)) => dispatch(command, player.as_ref()),
            Ok(None) => {}
            Err(error) => eprintln!("shared-memory MIDI receive failed: {error}"),
        }
    }
}

fn dispatch(command: FastMidiCommand, player: &dyn PlayerHandle) {
    let result = match command {
        FastMidiCommand::Midi {
            messages,
            offsets,
            patch,
        } => player.send_midi(messages, offsets, patch),
        FastMidiCommand::SetBufferMultiplier { multiplier } => {
            player.set_live_buffer_multiplier(multiplier)
        }
        FastMidiCommand::Stop => player.stop(),
    };
    if let Err(error) = result {
        eprintln!("shared-memory MIDI command failed: {error:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockPlayer {
        commands: Mutex<Vec<FastMidiCommand>>,
    }

    impl PlayerHandle for MockPlayer {
        fn play_smf(&self, _smf: Vec<u8>) -> Result<()> {
            Ok(())
        }

        fn play_mml(&self, _mml: String) -> Result<()> {
            Ok(())
        }

        fn send_midi(
            &self,
            messages: Vec<[u8; 3]>,
            offsets: Vec<u32>,
            patch: Option<String>,
        ) -> Result<()> {
            self.commands.lock().unwrap().push(FastMidiCommand::Midi {
                messages,
                offsets,
                patch,
            });
            Ok(())
        }

        fn prepare_live_patch(&self, _patch: Option<String>) -> Result<()> {
            Ok(())
        }

        fn set_live_buffer_multiplier(&self, multiplier: u8) -> Result<()> {
            self.commands
                .lock()
                .unwrap()
                .push(FastMidiCommand::SetBufferMultiplier { multiplier });
            Ok(())
        }

        fn stop(&self) -> Result<()> {
            self.commands.lock().unwrap().push(FastMidiCommand::Stop);
            Ok(())
        }
    }

    #[test]
    fn dispatch_uses_existing_player_interface() {
        let player = MockPlayer::default();
        let midi = FastMidiCommand::Midi {
            messages: vec![[0x90, 60, 100]],
            offsets: vec![5538],
            patch: Some("Keys/Piano.fxp".to_string()),
        };
        dispatch(midi.clone(), &player);
        dispatch(
            FastMidiCommand::SetBufferMultiplier { multiplier: 4 },
            &player,
        );
        dispatch(FastMidiCommand::Stop, &player);

        assert_eq!(
            *player.commands.lock().unwrap(),
            vec![
                midi,
                FastMidiCommand::SetBufferMultiplier { multiplier: 4 },
                FastMidiCommand::Stop
            ]
        );
    }
}
