use std::sync::Mutex;

use super::*;

/// chain の JSON を gain（`"0.5"` なら 0.5 倍）として読む偽の factory。`"broken"` は生成に失敗する。
#[derive(Default)]
struct GainFactory {
    built: Mutex<Vec<String>>,
}

struct Gain(f32);

impl InstancePostProcess for Gain {
    fn process(&mut self, samples: &mut [f32], _timing: &ProcessBlockTiming) -> Result<(), String> {
        for sample in samples {
            *sample *= self.0;
        }
        Ok(())
    }
}

impl LiveChainFactory for GainFactory {
    fn build(
        &self,
        chain_json: &str,
        _sample_rate: f64,
        _buf_size: usize,
    ) -> Result<Option<Box<dyn InstancePostProcess>>, String> {
        self.built.lock().unwrap().push(chain_json.to_string());
        if chain_json == "broken" {
            return Err("broken chain".to_string());
        }
        let gain: f32 = chain_json.parse().map_err(|_| "not a gain".to_string())?;
        Ok(Some(Box::new(Gain(gain))))
    }
}

fn chains(factory: &Arc<GainFactory>) -> InstanceChains {
    let source = ChainSource {
        factory: Arc::clone(factory) as Arc<dyn LiveChainFactory>,
        sample_rate: 48_000.0,
    };
    InstanceChains::new(0, source, 4, 2)
}

fn job(local_index: usize, patch: &str, chain: &str) -> PatchJob {
    PatchJob {
        local_index,
        patch: Some(patch.to_string()),
        effect_chain: chain.to_string(),
        reset_before: true,
        settle: true,
    }
}

/// 偽の `switch_patch`。呼ばれた回数を数える。
fn counting_load(count: &mut usize) -> impl FnOnce() -> PatchOutcome<()> + '_ {
    move || {
        *count += 1;
        PatchOutcome {
            swapped: false,
            result: Ok(()),
        }
    }
}

fn output(chains: &mut InstanceChains, local_index: usize) -> Vec<f32> {
    let timing = ProcessBlockTiming {
        steady_time: 0,
        transport: None,
    };
    chains
        .apply(local_index, Ok(vec![0.8; 8]), &timing)
        .unwrap()
}

/// 準備に同梱した chain が、その instance の出力にだけ掛かる。
#[test]
fn the_bundled_chain_applies_to_its_instance() {
    let factory = Arc::new(GainFactory::default());
    let mut chains = chains(&factory);
    let mut loads = 0;

    let outcome = chains.prepare(&job(1, "a.fxp", "0.5"), counting_load(&mut loads));

    assert_eq!(outcome.result, Ok(()));
    assert_eq!(loads, 1, "最初の準備は音色も読む");
    assert_eq!(output(&mut chains, 1), vec![0.4; 8]);
    assert_eq!(output(&mut chains, 0), vec![0.8; 8]);
}

/// chain が空なら factory を呼ばず（catalog の走査も起きない）、出力は従来どおり。
#[test]
fn an_empty_chain_keeps_the_dry_path() {
    let factory = Arc::new(GainFactory::default());
    let mut chains = chains(&factory);
    let mut loads = 0;

    chains.prepare(&job(0, "a.fxp", ""), counting_load(&mut loads));
    chains.prepare(&job(0, "a.fxp", ""), counting_load(&mut loads));

    assert_eq!(loads, 2, "chain 無しの準備は従来どおり毎回音色を読む");
    assert!(factory.built.lock().unwrap().is_empty());
    assert_eq!(output(&mut chains, 0), vec![0.8; 8]);
}

/// 音色が同じで chain だけ変わった要求では `switch_patch` を呼ばない。
/// 音色も変われば読む。chain を空にすると外れる。
#[test]
fn changing_only_the_chain_does_not_reload_the_patch() {
    let factory = Arc::new(GainFactory::default());
    let mut chains = chains(&factory);
    let mut loads = 0;
    chains.prepare(&job(0, "a.fxp", "0.5"), counting_load(&mut loads));
    assert_eq!(loads, 1);

    let mut chain_only_loads = 0;
    let outcome = chains.prepare(
        &job(0, "a.fxp", "0.25"),
        counting_load(&mut chain_only_loads),
    );
    assert_eq!(outcome.result, Ok(()));
    assert_eq!(
        chain_only_loads, 0,
        "chain だけの変更で switch_patch を呼んだ"
    );
    assert_eq!(output(&mut chains, 0), vec![0.2; 8]);

    let mut removed_loads = 0;
    chains.prepare(&job(0, "a.fxp", ""), counting_load(&mut removed_loads));
    assert_eq!(removed_loads, 0);
    assert_eq!(output(&mut chains, 0), vec![0.8; 8]);

    let mut patch_loads = 0;
    chains.prepare(&job(0, "b.fxp", "0.5"), counting_load(&mut patch_loads));
    assert_eq!(patch_loads, 1, "音色が変われば読む");
    assert_eq!(output(&mut chains, 0), vec![0.4; 8]);
}

/// probe で音色が変わった後は、chain だけが違う要求でも音色を読み直す。
#[test]
fn a_probe_forces_the_next_prepare_to_reload() {
    let factory = Arc::new(GainFactory::default());
    let mut chains = chains(&factory);
    let mut loads = 0;
    chains.prepare(&job(0, "a.fxp", "0.5"), counting_load(&mut loads));
    chains.forget_patch(0);

    chains.prepare(&job(0, "a.fxp", "0.25"), counting_load(&mut loads));

    assert_eq!(loads, 2);
}

/// chain を作れなければ準備は失敗し、音色は読まず、前の chain が残る。
#[test]
fn a_chain_that_fails_to_build_fails_the_prepare_without_loading() {
    let factory = Arc::new(GainFactory::default());
    let mut chains = chains(&factory);
    let mut loads = 0;
    chains.prepare(&job(0, "a.fxp", "0.5"), counting_load(&mut loads));

    let mut failed_loads = 0;
    let outcome = chains.prepare(&job(0, "b.fxp", "broken"), counting_load(&mut failed_loads));

    assert_eq!(outcome.result, Err("broken chain".to_string()));
    assert_eq!(failed_loads, 0);
    assert_eq!(output(&mut chains, 0), vec![0.4; 8]);
}
