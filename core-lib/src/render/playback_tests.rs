use super::*;
use cmrt_timeline::TransportTimeline;

fn tempo_map() -> Vec<SmfTempoChange> {
    vec![
        SmfTempoChange {
            at_seconds: 0.0,
            tempo_bpm: 120.0,
            numerator: 4,
            denominator: 4,
        },
        SmfTempoChange {
            at_seconds: 2.0,
            tempo_bpm: 60.0,
            numerator: 4,
            denominator: 4,
        },
    ]
}

#[test]
fn a_schedule_without_a_tempo_map_has_no_transport() {
    let schedule = RealtimePlaybackSchedule::new(Vec::new(), 100);
    assert!(schedule.transport.is_none());
    assert_eq!(schedule.musical_sample(), 0);
}

#[test]
fn the_tempo_map_becomes_a_piecewise_transport() {
    let schedule = RealtimePlaybackSchedule::with_tempo_map(Vec::new(), 100, &tempo_map(), 0);
    let transport = schedule.transport.expect("tempo map should build");

    let at = TimelineSeconds::new(4.0).unwrap();
    let snapshot = transport.snapshot_at(at).unwrap();
    assert_eq!(snapshot.tempo_bpm, 60.0);
    // 0..2 秒で 4 拍、そこから 2 秒で 2 拍。
    assert_eq!(snapshot.song_beats, 6.0);
}

/// preroll は描画後に切り落とす下駄。拍 0 は preroll のぶん後ろにある。
#[test]
fn the_musical_origin_absorbs_the_preroll() {
    let mut schedule =
        RealtimePlaybackSchedule::with_tempo_map(Vec::new(), 100_000, &tempo_map(), 4_800);
    assert_eq!(schedule.musical_sample(), 0, "preroll 中は拍 0 に留まる");

    schedule.current_sample = 4_800;
    assert_eq!(schedule.musical_sample(), 0, "曲の頭がちょうど拍 0");

    schedule.current_sample = 9_600;
    assert_eq!(schedule.musical_sample(), 4_800);
}

/// 変化点が巻き戻っていても、その1点を捨てて transport は生き残ること。
#[test]
fn a_backwards_change_point_is_dropped_without_losing_the_transport() {
    let mut map = tempo_map();
    map.push(SmfTempoChange {
        at_seconds: 1.0,
        tempo_bpm: 200.0,
        numerator: 4,
        denominator: 4,
    });
    let schedule = RealtimePlaybackSchedule::with_tempo_map(Vec::new(), 100, &map, 0);
    let transport = schedule.transport.expect("tempo map should build");

    let at = TimelineSeconds::new(4.0).unwrap();
    assert_eq!(transport.snapshot_at(at).unwrap().tempo_bpm, 60.0);
}
