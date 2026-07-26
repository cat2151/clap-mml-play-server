use super::*;

fn note_on(key: u8) -> [u8; 3] {
    [0x90, key, 100]
}

fn at_samples(queue: &[LiveQueuedEvent]) -> Vec<u64> {
    queue.iter().map(|queued| queued.at_sample).collect()
}

#[test]
fn enqueue_keeps_at_sample_ascending_across_batches() {
    let mut queue = Vec::new();
    enqueue_live_events(&mut queue, 1_000, &[note_on(60), note_on(64)], &[500, 100]);
    enqueue_live_events(&mut queue, 1_000, &[note_on(67)], &[300]);

    assert_eq!(at_samples(&queue), vec![1_100, 1_300, 1_500]);
    assert_eq!(queue[0].message, note_on(64));
    assert_eq!(queue[1].message, note_on(67));
    assert_eq!(queue[2].message, note_on(60));
}

#[test]
fn empty_offsets_place_every_message_at_the_current_clock() {
    let mut queue = Vec::new();
    enqueue_live_events(&mut queue, 4_096, &[note_on(60), note_on(64)], &[]);

    assert_eq!(at_samples(&queue), vec![4_096, 4_096]);
    // 同じ位置なら受け取った順を保つ（note off → note on の順序が崩れない）。
    assert_eq!(queue[0].message, note_on(60));
    assert_eq!(queue[1].message, note_on(64));
}

#[test]
fn enqueue_drops_events_beyond_the_queue_cap() {
    let mut queue = Vec::new();
    let messages = vec![note_on(60); MAX_LIVE_QUEUE_EVENTS + 10];

    enqueue_live_events(&mut queue, 0, &messages, &[]);

    assert_eq!(queue.len(), MAX_LIVE_QUEUE_EVENTS);
}

#[test]
fn take_chunk_events_maps_only_events_inside_the_chunk() {
    let mut queue = Vec::new();
    enqueue_live_events(&mut queue, 0, &[note_on(60), note_on(64)], &[10, 512]);

    let events = take_chunk_events(&mut queue, 0, 512);

    assert_eq!(
        events,
        vec![LiveMidiEvent {
            offset_frames: 10,
            message: note_on(60),
        }]
    );
    // 次チャンクぶんはキューに残る。
    assert_eq!(at_samples(&queue), vec![512]);

    let events = take_chunk_events(&mut queue, 512, 512);

    assert_eq!(
        events,
        vec![LiveMidiEvent {
            offset_frames: 0,
            message: note_on(64),
        }]
    );
    assert!(queue.is_empty());
}

#[test]
fn take_chunk_events_clamps_late_events_to_offset_zero() {
    let mut queue = vec![LiveQueuedEvent {
        at_sample: 100,
        message: note_on(60),
    }];

    let events = take_chunk_events(&mut queue, 1_024, 512);

    assert_eq!(
        events,
        vec![LiveMidiEvent {
            offset_frames: 0,
            message: note_on(60),
        }]
    );
    assert!(queue.is_empty());
}

#[test]
fn take_chunk_events_clamps_offsets_to_the_last_frame() {
    let mut queue = vec![LiveQueuedEvent {
        at_sample: 511,
        message: note_on(60),
    }];

    let events = take_chunk_events(&mut queue, 0, 512);

    assert_eq!(events[0].offset_frames, 511);
}
