use std::{sync::Arc, time::Duration};

use cmrt_core::{CoreConfig, RealtimeRenderer};
use cpal::traits::StreamTrait;

use super::{
    audio_output::{AudioOutputConsumer, AudioOutputControl, AudioOutputProducer},
    output_stream::build_output_stream,
    PlaybackMode, PlayerCommand, PlayerInner,
};

const OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(10);

pub(super) fn run_player_worker(
    inner: Arc<PlayerInner>,
    audio_output: Arc<AudioOutputControl>,
    core_cfg: CoreConfig,
    plugin_path: String,
    output_producer: AudioOutputProducer,
    output_consumer: AudioOutputConsumer,
    init_tx: std::sync::mpsc::Sender<std::result::Result<(), String>>,
) {
    let entry = match cmrt_core::load_entry(&plugin_path) {
        Ok(entry) => entry,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    let mut renderer = match RealtimeRenderer::new(&core_cfg, &entry) {
        Ok(renderer) => renderer,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    let output_stream = match build_output_stream(output_consumer, core_cfg.sample_rate) {
        Ok(stream) => stream,
        Err(error) => {
            let _ = init_tx.send(Err(format!("{error:#}")));
            return;
        }
    };
    if let Err(error) = output_stream.play() {
        let _ = init_tx.send(Err(format!("オーディオ出力の開始失敗: {error}")));
        return;
    }
    let _ = init_tx.send(Ok(()));

    let _keep_stream_alive = output_stream;
    let mut playback_mode: Option<PlaybackMode> = None;
    loop {
        if playback_mode.is_none() {
            let Some(command) = inner.wait_for_command() else {
                break;
            };
            apply_command(&mut renderer, &mut playback_mode, command);
            continue;
        }
        if let Some(command) = inner.pop_pending_command() {
            apply_command(&mut renderer, &mut playback_mode, command);
            continue;
        }
        if !output_producer.wait_for_space_timeout(OUTPUT_WAIT_TIMEOUT) {
            continue;
        }

        let Some(mode) = playback_mode.as_mut() else {
            continue;
        };
        match mode {
            PlaybackMode::Scheduled {
                generation,
                playback,
            } => match renderer.render_next_chunk(playback) {
                Ok(Some(chunk)) => {
                    if !output_producer.push_chunk(*generation, chunk) {
                        playback_mode = None;
                    }
                }
                Ok(None) => {
                    audio_output.finish();
                    playback_mode = None;
                }
                Err(error) => {
                    eprintln!("realtime play process failed: {error:#}");
                    renderer.reset();
                    audio_output.finish();
                    playback_mode = None;
                }
            },
            PlaybackMode::Live {
                generation,
                pending_messages,
            } => {
                let messages = std::mem::take(pending_messages);
                match renderer.render_live_chunk(&messages) {
                    Ok(chunk) => {
                        let _ = output_producer.push_chunk(*generation, chunk);
                    }
                    Err(error) => {
                        eprintln!("realtime live MIDI process failed: {error:#}");
                        renderer.reset();
                        audio_output.finish();
                        playback_mode = None;
                    }
                }
            }
        }
    }
}

fn apply_command(
    renderer: &mut RealtimeRenderer,
    playback_mode: &mut Option<PlaybackMode>,
    command: PlayerCommand,
) {
    match command {
        PlayerCommand::Play {
            generation,
            schedule,
            patch,
        } => {
            renderer.reset();
            if let Err(error) = renderer.set_patch(patch.as_deref()) {
                eprintln!("realtime play patch load failed: {error:#}");
            }
            *playback_mode = Some(PlaybackMode::Scheduled {
                generation,
                playback: schedule,
            });
        }
        PlayerCommand::Stop { generation } => {
            let _ = generation;
            renderer.reset();
            *playback_mode = None;
        }
        PlayerCommand::Midi {
            generation,
            messages,
            patch,
            enter_live,
        } => {
            if enter_live || !matches!(playback_mode, Some(PlaybackMode::Live { .. })) {
                renderer.reset();
                if let Err(error) = renderer.set_patch(patch.as_deref()) {
                    eprintln!("realtime live MIDI patch load failed: {error:#}");
                }
                *playback_mode = Some(PlaybackMode::Live {
                    generation,
                    pending_messages: Vec::new(),
                });
            }
            if let Some(PlaybackMode::Live {
                generation: live_generation,
                pending_messages,
            }) = playback_mode
            {
                *live_generation = generation;
                pending_messages.extend(messages);
            }
        }
        PlayerCommand::PrepareLivePatch {
            generation,
            patch,
            completion,
        } => {
            renderer.reset();
            match renderer.set_patch(patch.as_deref()) {
                Ok(()) => {
                    *playback_mode = Some(PlaybackMode::Live {
                        generation,
                        pending_messages: Vec::new(),
                    });
                    let _ = completion.send(Ok(()));
                }
                Err(error) => {
                    *playback_mode = None;
                    let _ = completion.send(Err(format!("{error:#}")));
                }
            }
        }
    }
}
