use super::*;
use cmrt_realtime_ipc::FastMidiEvent;

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
fn add_samples_sums_f32_stereo_buffers() {
    let mut mixed = vec![0.5, -0.25, 0.0, 1.0];
    add_samples(&mut mixed, &[0.25, 0.5, -0.5, 0.25]);
    assert_eq!(mixed, vec![0.75, 0.25, -0.5, 1.25]);
}
