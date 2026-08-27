//! 1 ブロックぶんの live 出力を作るところ。
//!
//! coordinator（[`super::run_player_worker`]）の仕事のうち「音を作る」部分だけを
//! ここに置く。CLAP を直接叩くのは bank worker で、ここは
//!
//! - イベントを instance ごとに配り、
//! - bank へ render を頼み、
//! - global instance index 順に合成する
//!
//! の 3 つしかしない。**mix 順（= 浮動小数の加算順）とゲインの位置をここに残している**
//! のは、bank 分離の前後で出力が 1 サンプルも変わらないようにするため。

use cmrt_clack_timeline::process_block_timing;
use cmrt_core::LiveMidiEvent;
use cmrt_timeline::{
    BlockEvent, BlockSpan, FreeRunningTimeline, LateEventPolicy, SamplePosition, SampleRate,
};

use super::{
    add_samples_ramped, take_chunk_events, AutoGainControl, BankWorkers, LiveGains,
    LiveInstanceState, LiveTimelineState, LiveTimingWindow, TimelinePayload,
};

pub(super) struct LiveMixControls<'a> {
    pub(super) gains: &'a LiveGains,
    pub(super) auto_gain: &'a AutoGainControl,
    pub(super) sample_rate: f64,
    pub(super) auto_gain_target_db: f32,
}

/// 1 ブロックぶんの live 出力を作る。
///
/// coordinator の仕事は「イベントを instance ごとに配る」「bank へ render を頼む」
/// 「global instance index 順に合成する」の 3 つで、CLAP を直接叩くのは bank worker。
/// mix 順（= 浮動小数の加算順）とゲインの位置をここに残しているのは、分離の前後で
/// 出力が 1 サンプルも変わらないようにするため。
pub(super) fn render_live_mix(
    banks: &BankWorkers,
    instances: &mut [LiveInstanceState],
    clock_samples: &mut u64,
    controls: LiveMixControls<'_>,
    timeline: Option<&mut LiveTimelineState>,
    timing_window: &mut LiveTimingWindow,
) -> anyhow::Result<Vec<f32>> {
    let auto_gain_enabled = controls.auto_gain.enabled();
    let buf_size = banks.buf_size() as u64;
    let chunk_start = *clock_samples;
    let timeline_sample_rate = SampleRate::new(controls.sample_rate)?;
    let block = BlockSpan::new(SamplePosition(chunk_start), buf_size as u32)?;
    let (scheduled, block_timing) = match timeline {
        Some(timeline) => {
            let scheduled = timeline
                .scheduler
                .take_block(block, LateEventPolicy::ClampToBlockStart);
            timing_window.observe_events(&scheduled);
            let timing = process_block_timing(block, timeline_sample_rate, &timeline.transport);
            (scheduled.events, timing)
        }
        None => (
            Vec::new(),
            process_block_timing(block, timeline_sample_rate, &FreeRunningTimeline),
        ),
    };
    let requests = live_render_requests(banks, instances, &scheduled, chunk_start, buf_size);
    let rendered = banks.render_instances(requests, block_timing)?;

    let mut mixed = vec![0.0f32; buf_size as usize * 2];
    for (index, (instance, outcome)) in instances.iter_mut().zip(rendered).enumerate() {
        let Some(outcome) = outcome else {
            continue;
        };
        match outcome {
            Ok(samples) => {
                let auto_gain = instance.auto_gain.process_block(
                    &samples,
                    controls.sample_rate,
                    controls.auto_gain_target_db,
                    auto_gain_enabled,
                );
                controls
                    .auto_gain
                    .set_gain_db(index, instance.auto_gain.gain_db());
                add_samples_ramped(
                    &mut mixed,
                    &samples,
                    auto_gain.scaled(controls.gains.get(index)),
                );
            }
            Err(error) => {
                eprintln!("realtime live instance {index} failed: {error}");
                // renderer の reset は所有している bank worker が済ませている。
                instance.active = false;
                instance.queue.clear();
                instance.auto_gain.reset();
                controls.auto_gain.set_gain_db(index, 0.0);
            }
        }
    }
    *clock_samples = chunk_start + buf_size;
    Ok(mixed)
}

/// instance ごとに「このブロックで鳴らすイベント」を組み立てる。
///
/// `None` は render 要求を出さない instance。次の 2 つが該当する。
///
/// - 非 active な instance
/// - **先読み中の bank に属する instance**。その bank worker は音色ロードで塞がっている。
///   ここで要求を出すと、ロードの後ろに render が並んで演奏が止まる。
///   キューは触らない（要求を出さないだけで、イベントを捨てはしない）。
fn live_render_requests(
    banks: &BankWorkers,
    instances: &mut [LiveInstanceState],
    scheduled: &[BlockEvent<TimelinePayload>],
    chunk_start: u64,
    buf_size: u64,
) -> Vec<Option<Vec<LiveMidiEvent>>> {
    let mut requests = Vec::with_capacity(instances.len());
    for (index, instance) in instances.iter_mut().enumerate() {
        if !instance.active {
            requests.push(None);
            continue;
        }
        if !banks.render_enabled_for(index) {
            // 鳴らすものがあるのに出せなかった。正規の経路では起きない（先読みは
            // 非演奏 bank へしか来ない）ので、数えて先読みの完了行へ出す。
            banks.note_render_skip(index);
            requests.push(None);
            continue;
        }
        let mut events = take_chunk_events(&mut instance.queue, chunk_start, buf_size);
        events.extend(
            scheduled
                .iter()
                .filter(|event| usize::from(event.payload.instance_id) == index)
                .map(|event| LiveMidiEvent {
                    offset_frames: event.offset_frames,
                    message: event.payload.message,
                }),
        );
        events.sort_by_key(|event| event.offset_frames);
        requests.push(Some(events));
    }
    requests
}
