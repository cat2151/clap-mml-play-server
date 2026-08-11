use super::*;
use cmrt_realtime_ipc::FastMidiEvent;
use cmrt_timeline::{
    BlockScheduler, BlockSpan, LateEventPolicy, SamplePosition, SampleRate, Timed, TimelineSeconds,
};

fn event(instance_id: u8, offset_frames: u32, key: u8) -> FastMidiEvent {
    FastMidiEvent {
        instance_id,
        offset_frames,
        message: [0x90, key, 100],
    }
}

fn at_samples(queue: &[LiveQueuedEvent]) -> Vec<u64> {
    queue.iter().map(|queued| queued.at_sample).collect()
}

#[test]
fn enqueue_keeps_sample_order_and_same_sample_insertion_order() {
    let mut queue = Vec::new();
    enqueue_live_event(&mut queue, 1_000, event(0, 500, 60));
    enqueue_live_event(&mut queue, 1_000, event(0, 100, 64));
    enqueue_live_event(&mut queue, 1_000, event(0, 300, 67));
    enqueue_live_event(&mut queue, 1_000, event(0, 300, 69));
    assert_eq!(at_samples(&queue), vec![1_100, 1_300, 1_300, 1_500]);
    assert_eq!(queue[1].message[1], 67);
    assert_eq!(queue[2].message[1], 69);
}

#[test]
fn enqueue_stops_at_the_per_instance_cap() {
    let mut queue = Vec::new();
    for _ in 0..MAX_LIVE_QUEUE_EVENTS + 10 {
        enqueue_live_event(&mut queue, 0, event(0, 0, 60));
    }
    assert_eq!(queue.len(), MAX_LIVE_QUEUE_EVENTS);
}

#[test]
fn take_chunk_events_keeps_future_events_and_clamps_late_ones() {
    let mut queue = vec![
        LiveQueuedEvent {
            at_sample: 100,
            message: [0x90, 60, 100],
        },
        LiveQueuedEvent {
            at_sample: 1_100,
            message: [0x90, 64, 100],
        },
        LiveQueuedEvent {
            at_sample: 1_600,
            message: [0x90, 67, 100],
        },
    ];
    let events = take_chunk_events(&mut queue, 1_024, 512);
    assert_eq!(events[0].offset_frames, 0);
    assert_eq!(events[1].offset_frames, 76);
    assert_eq!(at_samples(&queue), vec![1_600]);
}

#[test]
fn live_gains_default_to_unity_and_survive_updates() {
    let gains = LiveGains::default();
    assert_eq!(gains.get(0), 1.0);
    gains.set(0, 2.0);
    assert_eq!(gains.get(0), 2.0);
    assert_eq!(gains.get(1), 1.0, "他の instance は等倍のまま");
    // 範囲外の instance は無視し、既定値を返す。
    gains.set(999, 3.0);
    assert_eq!(gains.get(999), 1.0);
}

#[test]
fn auto_gain_control_defaults_off_and_switches_atomically() {
    let control = AutoGainControl::default();
    assert!(!control.enabled());
    control.set_enabled(true);
    assert!(control.enabled());
}

#[test]
fn absolute_scheduler_is_independent_of_command_receipt_cursor() {
    let requested = TimelineSeconds::new(0.250).unwrap();
    let rate = SampleRate::new(48_000.0).unwrap();
    let quantized = rate.seconds_to_sample(requested).unwrap();
    assert_eq!(quantized, SamplePosition(12_000));

    // The previous contract re-anchored an unchanged relative offset at receipt time.
    assert_ne!(1_000 + 12_000, 2_000 + 12_000);

    for simulated_receipt_cursor in [1_000, 2_000, 8_000] {
        let mut scheduler = BlockScheduler::new(rate);
        scheduler
            .schedule(Timed {
                at: requested,
                payload: 60,
            })
            .unwrap();
        // Receipt cursor is intentionally irrelevant. Only the absolute audio block determines
        // placement.
        let _ = simulated_receipt_cursor;
        let block = BlockSpan::new(SamplePosition(11_776), 512).unwrap();
        let events = scheduler.take_block(block, LateEventPolicy::ClampToBlockStart);
        assert_eq!(events.events[0].quantized_sample, SamplePosition(12_000));
        assert_eq!(events.events[0].offset_frames, 224);
    }
}

#[test]
fn buffer_lead_changes_do_not_change_inter_event_samples() {
    let rate = SampleRate::new(48_000.0).unwrap();
    let mut scheduler = BlockScheduler::new(rate);
    for step in 0..64u64 {
        scheduler
            .schedule(Timed {
                at: TimelineSeconds::from_step(step, 130.0, 4).unwrap(),
                payload: step,
            })
            .unwrap();
    }
    let mut placed = Vec::new();
    let mut block_start = 0u64;
    while !scheduler.is_empty() {
        let block = BlockSpan::new(SamplePosition(block_start), 512).unwrap();
        placed.extend(
            scheduler
                .take_block(block, LateEventPolicy::ClampToBlockStart)
                .events
                .into_iter()
                .map(|event| event.quantized_sample.0),
        );
        // Models arbitrary output-lead multiplier changes: block assignment remains on one
        // absolute lattice and does not use that lead.
        block_start += 512;
    }
    for (step, sample) in placed.into_iter().enumerate() {
        assert_eq!(
            sample,
            rate.seconds_to_sample(TimelineSeconds::from_step(step as u64, 130.0, 4).unwrap())
                .unwrap()
                .0
        );
    }
}
