use super::*;

fn audio_control() -> Arc<AudioOutputControl> {
    new_audio_output(512).0
}

#[test]
fn submit_play_replaces_older_pending_play() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner
        .submit_play(
            RealtimePlaybackSchedule::new(vec![], 10),
            Some("first.fxp".to_string()),
            Arc::clone(&audio_output),
        )
        .unwrap();
    inner
        .submit_play(
            RealtimePlaybackSchedule::new(vec![], 20),
            Some("second.fxp".to_string()),
            Arc::clone(&audio_output),
        )
        .unwrap();

    match inner.wait_for_command() {
        Some(PlayerCommand::Play {
            generation,
            schedule,
            patch,
        }) => {
            assert_eq!(generation, 2);
            assert_eq!(schedule.total_samples(), 20);
            assert_eq!(patch.as_deref(), Some("second.fxp"));
        }
        other => panic!("expected latest play command, got {other:?}"),
    }
}

#[test]
fn submit_stop_replaces_pending_play() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner
        .submit_play(
            RealtimePlaybackSchedule::new(vec![], 10),
            None,
            Arc::clone(&audio_output),
        )
        .unwrap();
    inner.submit_stop(Arc::clone(&audio_output)).unwrap();

    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::Stop { generation: 2 })
    ));
}

#[test]
fn live_midi_batches_keep_one_generation_and_fifo_order() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner
        .submit_midi(
            vec![[0x90, 60, 100]],
            Some("keys.fxp".to_string()),
            Arc::clone(&audio_output),
        )
        .unwrap();
    inner
        .submit_midi(
            vec![[0x90, 64, 100]],
            Some("ignored.fxp".to_string()),
            Arc::clone(&audio_output),
        )
        .unwrap();
    inner
        .submit_midi(vec![[0x80, 60, 0]], None, Arc::clone(&audio_output))
        .unwrap();

    assert_eq!(audio_output.generation(), 1);
    assert_midi_command(
        inner.wait_for_command(),
        &[[0x90, 60, 100]],
        Some("keys.fxp"),
        true,
    );
    assert_midi_command(
        inner.wait_for_command(),
        &[[0x90, 64, 100]],
        Some("ignored.fxp"),
        false,
    );
    assert_midi_command(inner.wait_for_command(), &[[0x80, 60, 0]], None, false);
    assert!(inner.pop_pending_command().is_none());
}

fn assert_midi_command(
    command: Option<PlayerCommand>,
    expected_messages: &[[u8; 3]],
    expected_patch: Option<&str>,
    expected_enter_live: bool,
) {
    match command {
        Some(PlayerCommand::Midi {
            generation,
            messages,
            patch,
            enter_live,
        }) => {
            assert_eq!(generation, 1);
            assert_eq!(messages, expected_messages);
            assert_eq!(patch.as_deref(), expected_patch);
            assert_eq!(enter_live, expected_enter_live);
        }
        other => panic!("expected MIDI command, got {other:?}"),
    }
}

#[test]
fn submit_play_discards_pending_live_midi() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner
        .submit_midi(vec![[0x90, 60, 100]], None, Arc::clone(&audio_output))
        .unwrap();
    inner
        .submit_play(
            RealtimePlaybackSchedule::new(vec![], 20),
            Some("song.fxp".to_string()),
            Arc::clone(&audio_output),
        )
        .unwrap();

    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::Play { generation: 2, .. })
    ));
    assert!(inner.pop_pending_command().is_none());
}

#[test]
fn submit_prepare_live_patch_replaces_pending_commands_and_returns_completion() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner
        .submit_midi(vec![[0x90, 60, 100]], None, Arc::clone(&audio_output))
        .unwrap();
    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
    inner
        .submit_prepare_live_patch(
            Some("keys.fxp".to_string()),
            completion_tx,
            Arc::clone(&audio_output),
        )
        .unwrap();

    match inner.wait_for_command() {
        Some(PlayerCommand::PrepareLivePatch {
            generation,
            patch,
            completion,
        }) => {
            assert_eq!(generation, 2);
            assert_eq!(patch.as_deref(), Some("keys.fxp"));
            completion.send(Ok(())).unwrap();
        }
        other => panic!("expected live patch command, got {other:?}"),
    }
    assert_eq!(completion_rx.recv().unwrap(), Ok(()));
    assert!(inner.pop_pending_command().is_none());
}

#[test]
fn resolve_live_patch_uses_patch_root_and_rejects_blank_patch() {
    let expected = Path::new("/surge-data")
        .join("patches_factory/Keys/Piano.fxp")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        resolve_live_patch(
            Some("patches_factory/Keys/Piano.fxp".to_string()),
            Some("/surge-data")
        )
        .as_deref(),
        Some(expected.as_str())
    );
    assert_eq!(
        resolve_live_patch(Some("  ".to_string()), Some("/patches")),
        None
    );
    assert_eq!(resolve_live_patch(None, Some("/patches")), None);
}

#[test]
fn submit_play_fails_after_shutdown() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner.shutdown(&audio_output);

    let error = inner
        .submit_play(
            RealtimePlaybackSchedule::new(vec![], 10),
            None,
            audio_control(),
        )
        .unwrap_err();

    assert!(error.to_string().contains("stopped"));
}

#[test]
fn wait_for_command_returns_none_after_shutdown() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner.shutdown(&audio_output);

    assert!(inner.wait_for_command().is_none());
}
