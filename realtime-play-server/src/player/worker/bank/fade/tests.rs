use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::super::instance_chain::ChainSource;
use super::super::live_chain::LiveChainFactory;
use super::super::post_process::InstancePostProcess;
use super::super::protocol::{PatchJob, PatchOutcome};
use super::*;

fn timing() -> ProcessBlockTiming {
    ProcessBlockTiming {
        steady_time: 0,
        transport: None,
    }
}

/// `frames` frame ぶんの interleaved stereo（全 sample が `level`）。
fn block(frames: usize, level: f32) -> Vec<f32> {
    vec![level; frames * 2]
}

/// 左 channel だけを取り出す（左右は同じ gain なので片側で足りる）。
fn left(samples: &[f32]) -> Vec<f32> {
    samples.iter().step_by(2).copied().collect()
}

fn note(offset_frames: u32, key: u8) -> LiveMidiEvent {
    LiveMidiEvent {
        offset_frames,
        message: [0x90, key, 100],
    }
}

/// fade を掛けた instance だけが `fade_frames` ちょうどで 0 になり、その後も 0 のまま。
#[test]
fn only_the_faded_instance_reaches_zero_in_the_fade_length() {
    let mut fades = InstanceFades::new(2);
    fades.start(0, 8);

    let mut first = block(6, 1.0);
    let mut other = block(6, 1.0);
    assert!(!fades.apply(0, &mut first));
    assert!(!fades.apply(1, &mut other));
    assert_eq!(left(&first), [0.875, 0.75, 0.625, 0.5, 0.375, 0.25]);
    assert_eq!(other, block(6, 1.0), "他の instance は変わらない");

    let mut second = block(4, 1.0);
    assert!(fades.apply(0, &mut second), "8 frame 目で 0 に達する");
    assert_eq!(left(&second), [0.125, 0.0, 0.0, 0.0]);

    let mut after = block(4, 1.0);
    assert!(!fades.apply(0, &mut after));
    assert_eq!(after, block(4, 0.0), "新しい行が来るまで 0 を保つ");
}

/// 絞った行の続きのイベントは捨てる。新しい行が始まった後のイベントが届いたら、そこから等倍で鳴る。
#[test]
fn the_old_line_is_dropped_and_the_new_line_sounds_at_full_gain() {
    let mut fades = InstanceFades::new(1);
    fades.start(0, 2);
    assert!(fades.apply(0, &mut block(2, 1.0)));

    assert!(fades.block_events(0, &[note(0, 60)]).is_empty(), "古い行");
    let mut silent = block(2, 1.0);
    fades.apply(0, &mut silent);
    assert_eq!(silent, block(2, 0.0));

    fades.begin_new_line_all();
    assert!(
        fades.block_events(0, &[]).is_empty(),
        "イベントが来るまでは 0 のまま"
    );
    let mut still_silent = block(2, 1.0);
    fades.apply(0, &mut still_silent);
    assert_eq!(still_silent, block(2, 0.0));

    assert_eq!(fades.block_events(0, &[note(1, 64)]), [note(1, 64)]);
    let mut sounding = block(2, 1.0);
    assert!(!fades.apply(0, &mut sounding));
    assert_eq!(sounding, block(2, 1.0));
}

/// ramp は sample 単位で、1 frame の変化が「元の音量 / 短い方の fade の長さ」を超えない。
/// fade の途中で重ねて要求しても、その時点の gain から絞り直す（段差を作らない）。
#[test]
fn the_ramp_has_no_step_even_when_restarted() {
    let level = 0.5;
    let fade_frames = 480;
    let max_step = level / 100.0 + 1e-6;
    let mut fades = InstanceFades::new(1);
    fades.start(0, fade_frames);
    let mut previous = level;
    let mut outputs = Vec::new();
    for index in 0.. {
        if index == 2 {
            fades.start(0, 100);
        }
        let mut samples = block(128, level);
        let silenced = fades.apply(0, &mut samples);
        outputs.extend(left(&samples));
        if silenced {
            break;
        }
    }
    for (frame, &sample) in outputs.iter().enumerate() {
        assert!(
            (previous - sample).abs() <= max_step,
            "frame {frame}: {previous} -> {sample}"
        );
        previous = sample;
    }
    assert_eq!(*outputs.last().unwrap(), 0.0);
}

/// fade 中に始まった新しい行のイベントは renderer へ渡さずに預かり、0 に達した次の block の
/// 頭へ置く。新しい行が始まる前のイベント（絞った行の続き）は預からない。
#[test]
fn events_during_the_fade_wait_until_it_ends() {
    let mut fades = InstanceFades::new(1);
    fades.start(0, 4);

    assert!(fades.block_events(0, &[note(1, 59)]).is_empty());
    fades.begin_new_line(0);
    assert!(fades.block_events(0, &[note(1, 60)]).is_empty());
    assert!(!fades.apply(0, &mut block(2, 1.0)));
    assert!(fades.block_events(0, &[note(1, 62)]).is_empty());
    assert!(fades.apply(0, &mut block(2, 1.0)));

    assert_eq!(
        fades.block_events(0, &[note(3, 64)]),
        [note(0, 60), note(0, 62), note(3, 64)]
    );
    assert_eq!(
        fades.block_events(0, &[note(5, 65)]),
        [note(5, 65)],
        "渡し終えたイベントは 2 度渡さない"
    );
}

/// 停止（reset_all）で fade も預かったイベントも捨てる。次の演奏を絞らない。
#[test]
fn clearing_drops_the_fade_and_the_held_events() {
    let mut fades = InstanceFades::new(1);
    fades.start(0, 4);
    assert!(fades.block_events(0, &[note(0, 60)]).is_empty());

    fades.clear_all();

    let mut samples = block(2, 1.0);
    assert!(!fades.apply(0, &mut samples));
    assert_eq!(samples, block(2, 1.0));
    assert_eq!(fades.block_events(0, &[note(1, 62)]), [note(1, 62)]);
}

/// reset された回数を数える偽の chain（素通し）。
struct CountingChain(Arc<AtomicUsize>);

impl InstancePostProcess for CountingChain {
    fn process(
        &mut self,
        _samples: &mut [f32],
        _timing: &ProcessBlockTiming,
    ) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct CountingFactory(Arc<AtomicUsize>);

impl LiveChainFactory for CountingFactory {
    fn build(
        &self,
        _chain_json: &str,
        _sample_rate: f64,
        _buf_size: usize,
    ) -> Result<Option<Box<dyn InstancePostProcess>>, String> {
        Ok(Some(Box::new(CountingChain(Arc::clone(&self.0)))))
    }
}

fn chains_with_a_chain_on_instance_0(resets: &Arc<AtomicUsize>) -> InstanceChains {
    let source = ChainSource {
        factory: Arc::new(CountingFactory(Arc::clone(resets))),
        sample_rate: 48_000.0,
    };
    let mut chains = InstanceChains::new(0, source, 2, 1);
    let job = PatchJob {
        local_index: 0,
        patch: Some("patch".to_string()),
        effect_chain: "[chain]".to_string(),
        reset_before: true,
        settle: true,
    };
    let outcome = chains.prepare(&job, || PatchOutcome {
        swapped: false,
        result: Ok(()),
    });
    assert!(outcome.result.is_ok());
    chains
}

/// fade が 0 に達した block で、instance の voice と chain の状態を捨てる。達する前は捨てない。
#[test]
fn reaching_zero_resets_the_voices_and_the_chain() {
    let chain_resets = Arc::new(AtomicUsize::new(0));
    let mut chains = chains_with_a_chain_on_instance_0(&chain_resets);
    let mut fades = InstanceFades::new(1);
    let mut voice_resets = 0;
    fades.start(0, 4);

    let first = finish_instance_output(
        &mut chains,
        &mut fades,
        0,
        Ok(block(2, 1.0)),
        &timing(),
        || voice_resets += 1,
    )
    .unwrap();
    assert_eq!(left(&first), [0.75, 0.5]);
    assert_eq!((voice_resets, chain_resets.load(Ordering::SeqCst)), (0, 0));

    let second = finish_instance_output(
        &mut chains,
        &mut fades,
        0,
        Ok(block(2, 1.0)),
        &timing(),
        || voice_resets += 1,
    )
    .unwrap();
    assert_eq!(left(&second), [0.25, 0.0]);
    assert_eq!((voice_resets, chain_resets.load(Ordering::SeqCst)), (1, 1));

    let third = finish_instance_output(
        &mut chains,
        &mut fades,
        0,
        Ok(block(2, 1.0)),
        &timing(),
        || voice_resets += 1,
    )
    .unwrap();
    assert_eq!(third, block(2, 0.0), "reset で消えない余韻も出さない");
    assert_eq!((voice_resets, chain_resets.load(Ordering::SeqCst)), (1, 1));
}

/// render が失敗した instance は、従来どおり voice と chain を捨て、fade も捨てる。
#[test]
fn a_failed_render_resets_and_drops_the_fade() {
    let chain_resets = Arc::new(AtomicUsize::new(0));
    let mut chains = chains_with_a_chain_on_instance_0(&chain_resets);
    let mut fades = InstanceFades::new(1);
    let mut voice_resets = 0;
    fades.start(0, 4);

    let failed = finish_instance_output(
        &mut chains,
        &mut fades,
        0,
        Err("render failed".to_string()),
        &timing(),
        || voice_resets += 1,
    );

    assert!(failed.is_err());
    assert_eq!((voice_resets, chain_resets.load(Ordering::SeqCst)), (1, 1));
    let mut samples = block(2, 1.0);
    assert!(!fades.apply(0, &mut samples));
    assert_eq!(samples, block(2, 1.0));
}
