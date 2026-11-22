use cmrt_timeline::{BlockSpan, LateEventPolicy, SamplePosition};

use super::*;

#[test]
fn patch_cc1_reset_replaces_pending_modulation_for_only_the_loaded_instance() {
    let mut timeline = LiveTimelineState::new(LiveTimelineConfig {
        timeline_id: 1,
        sample_rate_hz: 48_000.0,
        tempo_bpm: 120.0,
        time_signature_numerator: 4,
        time_signature_denominator: 4,
    })
    .unwrap();
    let messages = [
        (0, [0xb0, 1, 127]),
        (0, [0xb0, 7, 80]),
        (0, [0x90, 60, 100]),
        (1, [0xb0, 1, 100]),
    ];
    for (instance_id, message) in messages {
        timeline
            .schedule(TimelineMidiEvent {
                timeline_id: 1,
                timeline_seconds: 0.0,
                instance_id,
                message,
            })
            .unwrap();
    }

    let mut instance = LiveInstanceState::default();
    instance.reset_patch_midi(Some(&mut timeline), 0, 32, false);
    assert!(instance.queue.is_empty());
    assert_eq!(timeline.scheduler.len(), messages.len());

    instance.reset_patch_midi(Some(&mut timeline), 0, 32, true);
    assert_eq!(
        instance.queue,
        [LiveQueuedEvent {
            at_sample: 32,
            message: [0xb0, 1, 0],
        }]
    );
    let block = BlockSpan::new(SamplePosition(0), 512).unwrap();
    let remaining = timeline.scheduler.take_block(block, LateEventPolicy::Drop);
    assert_eq!(
        remaining
            .events
            .into_iter()
            .map(|event| (event.payload.instance_id, event.payload.message))
            .collect::<Vec<_>>(),
        messages[1..]
    );
}
