use clack_host::events::event_types::TransportFlags;
use cmrt_timeline::{
    BlockSpan, ConstantTempoTimeline, FreeRunningTimeline, SamplePosition, SampleRate,
};

use super::*;

#[test]
fn free_running_blocks_still_supply_monotonic_steady_time() {
    let rate = SampleRate::new(48_000.0).unwrap();
    let first = process_block_timing(
        BlockSpan::new(SamplePosition(512), 256).unwrap(),
        rate,
        &FreeRunningTimeline,
    );
    let second = process_block_timing(
        BlockSpan::new(SamplePosition(768), 256).unwrap(),
        rate,
        &FreeRunningTimeline,
    );
    assert_eq!(first.steady_time, 512);
    assert_eq!(second.steady_time, 768);
    assert!(first.transport.is_none());
}

#[test]
fn constant_tempo_maps_to_clap_transport() {
    let rate = SampleRate::new(48_000.0).unwrap();
    let timeline = ConstantTempoTimeline::new(120.0, 4, 4).unwrap();
    let timing = process_block_timing(
        BlockSpan::new(SamplePosition(240_000), 512).unwrap(),
        rate,
        &timeline,
    );
    let transport = timing.transport.unwrap();
    assert!(transport.flags.contains(TransportFlags::IS_PLAYING));
    assert_eq!(transport.tempo, 120.0);
    assert_eq!(transport.song_pos_beats.to_float(), 10.0);
    assert_eq!(transport.song_pos_seconds.to_float(), 5.0);
    assert_eq!(transport.bar_number, 2);
}
