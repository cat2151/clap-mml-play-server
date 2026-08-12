use super::*;

fn seconds(value: f64) -> TimelineSeconds {
    TimelineSeconds::new(value).unwrap()
}

fn map(tempo_bpm: f64) -> TempoMapTimeline {
    TempoMapTimeline::new(tempo_bpm, 4, 4).unwrap()
}

#[test]
fn invalid_tempo_and_time_signature_are_rejected() {
    for tempo in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        assert!(TempoMapTimeline::new(tempo, 4, 4).is_none());
        assert!(ConstantTempoTimeline::new(tempo, 4, 4).is_none());
    }
    for (numerator, denominator) in [(0u16, 4u16), (4, 0), (4, 3), (4, 6)] {
        assert!(TempoMapTimeline::new(120.0, numerator, denominator).is_none());
        assert!(ConstantTempoTimeline::new(120.0, numerator, denominator).is_none());
    }

    let mut timeline = map(120.0);
    assert_eq!(
        timeline.push(seconds(1.0), 0.0, 4, 4),
        Err(TempoMapError::InvalidTempo(0.0))
    );
    assert_eq!(
        timeline.push(seconds(1.0), 120.0, 4, 3),
        Err(TempoMapError::InvalidTimeSignature {
            numerator: 4,
            denominator: 3,
        })
    );
    assert_eq!(timeline.segments().len(), 1);
}

/// 1区間だけの tempo map は、既存の定テンポ実装と完全に一致すること。
#[test]
fn a_single_segment_matches_the_constant_tempo_timeline() {
    let constant = ConstantTempoTimeline::new(130.0, 4, 4).unwrap();
    let tempo_map = map(130.0);
    for step in 0..2_000u64 {
        let at = TimelineSeconds::from_step(step, 130.0, 4).unwrap();
        assert_eq!(tempo_map.snapshot_at(at), constant.snapshot_at(at));
    }
}

/// BPM120 で10秒進むと20拍。そこから BPM60 に変えて更に10秒進むと合計30拍。
#[test]
fn beats_accumulate_piecewise_across_a_tempo_change() {
    let mut timeline = map(120.0);
    assert_eq!(
        timeline.snapshot_at(seconds(10.0)).unwrap().song_beats,
        20.0
    );

    timeline.push(seconds(10.0), 60.0, 4, 4).unwrap();
    let snapshot = timeline.snapshot_at(seconds(20.0)).unwrap();
    assert_eq!(snapshot.song_beats, 30.0);
    assert_eq!(snapshot.tempo_bpm, 60.0);
    // 変化点より前を訊かれたら、これまでどおり変更前のテンポで答える。
    let before = timeline.snapshot_at(seconds(5.0)).unwrap();
    assert_eq!(before.song_beats, 10.0);
    assert_eq!(before.tempo_bpm, 120.0);
}

/// 区間の境界はその区間の側へ倒す（境界のブロックが既に新テンポで鳴るように）。
#[test]
fn a_segment_boundary_belongs_to_the_new_segment() {
    let mut timeline = map(120.0);
    timeline.push(seconds(10.0), 60.0, 4, 4).unwrap();
    let at_boundary = timeline.snapshot_at(seconds(10.0)).unwrap();
    assert_eq!(at_boundary.tempo_bpm, 60.0);
    assert_eq!(at_boundary.song_beats, 20.0);
    let just_before = timeline.snapshot_at(seconds(10.0 - 1e-9)).unwrap();
    assert_eq!(just_before.tempo_bpm, 120.0);
}

#[test]
fn pushing_before_the_last_segment_is_rejected() {
    let mut timeline = map(120.0);
    timeline.push(seconds(10.0), 60.0, 4, 4).unwrap();
    assert_eq!(
        timeline.push(seconds(9.0), 90.0, 4, 4),
        Err(TempoMapError::NotMonotonic {
            at: seconds(9.0),
            last: seconds(10.0),
        })
    );
    assert_eq!(timeline.segments().len(), 2);
    // 拒否しても、それまでの答えは一切動かない。
    assert_eq!(
        timeline.snapshot_at(seconds(20.0)).unwrap().song_beats,
        30.0
    );
}

#[test]
fn an_unchanged_tempo_does_not_add_a_segment() {
    let mut timeline = map(120.0);
    timeline.push(seconds(10.0), 120.0, 4, 4).unwrap();
    timeline.push(seconds(20.0), 120.0, 4, 4).unwrap();
    assert_eq!(timeline.segments().len(), 1);
    // 拍子だけが変われば区間は増える。
    timeline.push(seconds(20.0), 120.0, 3, 4).unwrap();
    assert_eq!(timeline.segments().len(), 2);
}

#[test]
fn pushing_at_the_same_time_replaces_the_last_segment() {
    let mut timeline = map(120.0);
    timeline.push(seconds(10.0), 60.0, 4, 4).unwrap();
    timeline.push(seconds(10.0), 90.0, 4, 4).unwrap();
    assert_eq!(timeline.segments().len(), 2);
    let snapshot = timeline.snapshot_at(seconds(10.0)).unwrap();
    assert_eq!(snapshot.tempo_bpm, 90.0);
    // 差し替えても、変化点までの積算拍は変わらない。
    assert_eq!(snapshot.song_beats, 20.0);
}

/// 上限を超えたら最も古い区間から落ちる。現在位置（末尾側）の答えは変わらない。
#[test]
fn the_segment_list_is_bounded_and_drops_the_oldest() {
    let mut timeline = map(120.0);
    for index in 1..=(MAX_TEMPO_SEGMENTS * 3) {
        let tempo = if index % 2 == 0 { 120.0 } else { 60.0 };
        timeline.push(seconds(index as f64), tempo, 4, 4).unwrap();
    }
    assert_eq!(timeline.segments().len(), MAX_TEMPO_SEGMENTS);
    // 上限に達しても再確保していない（レンダースレッドで確保しないための要）。
    assert_eq!(timeline.segments.capacity(), MAX_TEMPO_SEGMENTS);

    let last = *timeline.segments().last().unwrap();
    let snapshot = timeline.snapshot_at(last.start_seconds).unwrap();
    assert_eq!(snapshot.tempo_bpm, last.tempo_bpm);
    assert_eq!(snapshot.song_beats, last.start_beats);
    // 落とした範囲を訊かれても、最古の区間で答えるだけで壊れない。
    assert!(timeline.snapshot_at(TimelineSeconds::ZERO).is_some());
}

#[test]
fn bars_follow_the_current_time_signature() {
    let mut timeline = map(120.0);
    // BPM120 の 4/4 では 1小節 = 2秒。
    let snapshot = timeline.snapshot_at(seconds(5.0)).unwrap();
    assert_eq!(snapshot.bar_number, 2);
    assert_eq!(snapshot.bar_start_beats, 8.0);

    timeline.push(seconds(8.0), 120.0, 3, 4).unwrap();
    let snapshot = timeline.snapshot_at(seconds(8.0)).unwrap();
    assert_eq!(snapshot.time_signature_numerator, 3);
    assert_eq!(snapshot.song_beats, 16.0);
    assert_eq!(snapshot.bar_number, 5);
}

/// 秒→拍は単調増加でなければならない。巻き戻るとサーバー側が鳴らし損ねる。
#[test]
fn beats_never_move_backwards_across_many_tempo_changes() {
    let mut timeline = map(130.0);
    let mut at = 0.0;
    for index in 0..(MAX_TEMPO_SEGMENTS * 4) {
        at += 7.4;
        let tempo = 80.0 + f64::from(u32::try_from(index % 81).unwrap());
        timeline.push(seconds(at), tempo, 4, 4).unwrap();
    }
    let mut previous = f64::NEG_INFINITY;
    let mut probe = timeline.segments()[0].start_seconds.get();
    while probe <= at + 10.0 {
        let beats = timeline.snapshot_at(seconds(probe)).unwrap().song_beats;
        assert!(beats >= previous, "{previous} -> {beats} at {probe}");
        previous = beats;
        probe += 0.05;
    }
}
