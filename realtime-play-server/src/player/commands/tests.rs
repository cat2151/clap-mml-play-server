use super::*;
use crate::player::audio_output::new_audio_output;

fn audio_control() -> Arc<AudioOutputControl> {
    new_audio_output(512).0
}

fn event(instance_id: InstanceId, note: u8) -> FastMidiEvent {
    FastMidiEvent {
        instance_id,
        offset_frames: 0,
        message: [0x90, note, 100],
    }
}

#[test]
fn submit_play_and_stop_replace_pending_work() {
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
    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::Play {
            generation: 2,
            patch: Some(patch),
            ..
        }) if patch == "second.fxp"
    ));

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
        Some(PlayerCommand::StopAll { generation: 4 })
    ));
}

#[test]
fn live_batches_share_generation_and_preserve_instance_ids() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner
        .submit_midi(vec![event(0, 60), event(15, 72)], Arc::clone(&audio_output))
        .unwrap();
    inner
        .submit_midi(vec![event(7, 64)], Arc::clone(&audio_output))
        .unwrap();

    assert_eq!(audio_output.generation(), 1);
    match inner.wait_for_command() {
        Some(PlayerCommand::Midi {
            generation,
            events,
            enter_live,
        }) => {
            assert_eq!(generation, 1);
            assert!(enter_live);
            assert_eq!(events[0].instance_id, 0);
            assert_eq!(events[1].instance_id, 15);
        }
        other => panic!("expected MIDI command, got {other:?}"),
    }
    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::Midi {
            generation: 1,
            enter_live: false,
            ..
        })
    ));
}

#[test]
fn prepare_patch_targets_one_instance_and_returns_completion() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
    inner
        .submit_prepare_live_patch(
            12,
            Some("keys.fxp".to_string()),
            completion_tx,
            Arc::clone(&audio_output),
        )
        .unwrap();

    match inner.wait_for_command() {
        Some(PlayerCommand::PrepareLivePatch {
            generation,
            instance_id,
            patch,
            completion,
        }) => {
            assert_eq!(generation, 1);
            assert_eq!(instance_id, 12);
            assert_eq!(patch.as_deref(), Some("keys.fxp"));
            completion.send(Ok(())).unwrap();
        }
        other => panic!("expected patch command, got {other:?}"),
    }
    assert_eq!(completion_rx.recv().unwrap(), Ok(()));
}

/// live 中の patch 差し替えで generation を上げると、リング内の描画済みフレームが
/// 全部捨てられて鳴っている instance の音が飛ぶ。grid sequencer の chord mode は
/// 演奏の裏で次の bank を仕込むので、据え置きでなければならない。
#[test]
fn preparing_a_patch_while_live_keeps_the_generation() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner
        .submit_midi(vec![event(0, 60)], Arc::clone(&audio_output))
        .unwrap();
    assert_eq!(audio_output.generation(), 1);
    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::Midi { generation: 1, .. })
    ));

    let (completion_tx, _completion_rx) = std::sync::mpsc::sync_channel(1);
    inner
        .submit_prepare_live_patch(
            9,
            Some("shadow.fxp".to_string()),
            completion_tx,
            Arc::clone(&audio_output),
        )
        .unwrap();

    assert_eq!(
        audio_output.generation(),
        1,
        "リングの中身を捨てさせないため generation は据え置く"
    );
    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::PrepareLivePatch {
            generation: 1,
            instance_id: 9,
            ..
        })
    ));
}

/// live に入る前の prepare は従来どおり generation を進める（起動時・`r` キーの経路）。
#[test]
fn preparing_a_patch_before_going_live_starts_a_new_generation() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    let (completion_tx, _completion_rx) = std::sync::mpsc::sync_channel(1);
    inner
        .submit_prepare_live_patch(
            0,
            Some("keys.fxp".to_string()),
            completion_tx,
            Arc::clone(&audio_output),
        )
        .unwrap();

    assert_eq!(audio_output.generation(), 1);
    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::PrepareLivePatch { generation: 1, .. })
    ));
}

#[test]
fn stop_instance_does_not_clear_queued_commands_for_other_instances() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner
        .submit_midi(vec![event(0, 60)], Arc::clone(&audio_output))
        .unwrap();
    inner
        .submit_stop_instance(0, Arc::clone(&audio_output))
        .unwrap();

    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::Midi { .. })
    ));
    assert!(matches!(
        inner.wait_for_command(),
        Some(PlayerCommand::StopInstance {
            instance_id: 0,
            generation: 2
        })
    ));
}

#[test]
fn commands_fail_after_shutdown() {
    let inner = PlayerInner::default();
    let audio_output = audio_control();
    inner.shutdown(&audio_output);
    assert!(inner
        .submit_play(
            RealtimePlaybackSchedule::new(vec![], 10),
            None,
            audio_control()
        )
        .is_err());
    assert!(inner.wait_for_command().is_none());
}
