use super::*;

#[test]
fn invalid_clock_values_are_rejected() {
    for value in [f64::NAN, f64::INFINITY, -0.1] {
        assert!(TimelineSeconds::new(value).is_err());
    }
    for value in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        assert!(SampleRate::new(value).is_err());
    }
}

#[test]
fn one_hour_of_steps_does_not_accumulate_time_error() {
    let sample_rate = SampleRate::new(192_000.0).unwrap();
    let steps = 31_200u64;
    let mut previous = TimelineSeconds::ZERO;
    for step in 0..=steps {
        let at = TimelineSeconds::from_step(step, 130.0, 4).unwrap();
        assert!(at >= previous);
        let ideal = step as f64 * 60.0 / 520.0;
        assert!((at.get() - ideal).abs() * sample_rate.get() < 0.001);
        previous = at;
    }
    assert_eq!(previous.get(), 3_600.0);
}

#[test]
fn quantization_stays_within_half_a_sample() {
    for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        let sample_rate = SampleRate::new(rate).unwrap();
        for step in 0..10_000 {
            let at = TimelineSeconds::from_step(step, 130.0, 4).unwrap();
            let sample = sample_rate.seconds_to_sample(at).unwrap();
            let recovered = sample_rate.sample_to_seconds(sample);
            assert!((recovered.get() - at.get()).abs() * rate <= 0.5 + f64::EPSILON);
        }
    }
}

#[test]
fn scheduler_keeps_equal_time_events_stable_and_assigns_offsets() {
    let rate = SampleRate::new(48_000.0).unwrap();
    let mut scheduler = BlockScheduler::new(rate);
    for payload in ['a', 'b', 'c'] {
        scheduler
            .schedule(Timed {
                at: TimelineSeconds::new(513.0 / 48_000.0).unwrap(),
                payload,
            })
            .unwrap();
    }
    let block = BlockSpan::new(SamplePosition(512), 512).unwrap();
    let events = scheduler.take_block(block, LateEventPolicy::ClampToBlockStart);
    assert_eq!(
        events
            .events
            .iter()
            .map(|event| event.payload)
            .collect::<Vec<_>>(),
        vec!['a', 'b', 'c']
    );
    assert!(events.events.iter().all(|event| event.offset_frames == 1));
}

#[test]
fn late_policy_is_explicit() {
    let rate = SampleRate::new(48_000.0).unwrap();
    let at = TimelineSeconds::new(100.0 / 48_000.0).unwrap();
    let block = BlockSpan::new(SamplePosition(512), 512).unwrap();

    let mut clamped = BlockScheduler::new(rate);
    clamped.schedule(Timed { at, payload: 1 }).unwrap();
    let result = clamped.take_block(block, LateEventPolicy::ClampToBlockStart);
    assert_eq!(result.events[0].offset_frames, 0);
    assert_eq!(result.events[0].late_by_samples, 412);

    let mut dropped = BlockScheduler::new(rate);
    dropped.schedule(Timed { at, payload: 1 }).unwrap();
    let result = dropped.take_block(block, LateEventPolicy::Drop);
    assert!(result.events.is_empty());
    assert_eq!(result.dropped_late_events, 1);
}

#[test]
fn constant_tempo_reports_beats_and_bars() {
    let timeline = ConstantTempoTimeline::new(120.0, 4, 4).unwrap();
    let snapshot = timeline
        .snapshot_at(TimelineSeconds::new(5.0).unwrap())
        .unwrap();
    assert_eq!(snapshot.song_beats, 10.0);
    assert_eq!(snapshot.bar_number, 2);
    assert_eq!(snapshot.bar_start_beats, 8.0);
}

#[test]
fn diagnostics_keep_window_and_total_separate() {
    let mut diagnostics = TimingDiagnostics::default();
    diagnostics.observe(PlacementObservation {
        quantization_error_seconds: -0.000_001,
        late_by_samples: 12,
        placement_error_samples: -2,
    });
    assert_eq!(diagnostics.take_window().late_events, 1);
    assert_eq!(diagnostics.window(), TimingSnapshot::default());
    assert_eq!(diagnostics.total().max_late_by_samples, 12);
}

#[test]
fn offline_and_live_callers_receive_identical_block_offsets() {
    let rate = SampleRate::new(48_000.0).unwrap();
    let events = (0..32u64)
        .map(|step| Timed {
            at: TimelineSeconds::from_step(step, 130.0, 4).unwrap(),
            payload: step,
        })
        .collect::<Vec<_>>();
    let mut offline = BlockScheduler::new(rate);
    let mut live = BlockScheduler::new(rate);
    for event in events {
        offline.schedule(event.clone()).unwrap();
        live.schedule(event).unwrap();
    }

    let mut cursor = 0u64;
    while !offline.is_empty() {
        let block = BlockSpan::new(SamplePosition(cursor), 512).unwrap();
        let offline_block = offline.take_block(block, LateEventPolicy::ClampToBlockStart);
        let live_block = live.take_block(block, LateEventPolicy::ClampToBlockStart);
        assert_eq!(offline_block, live_block);
        cursor += 512;
    }
}
