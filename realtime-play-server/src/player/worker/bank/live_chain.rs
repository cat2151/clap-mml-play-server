//! live instance の出力に掛ける effect chain を、音色の準備に同梱された JSON から作る。
//!
//! effect の catalog の走査も effect plugin の DLL ロードも、最初に chain 付きの要求が
//! 来るまで遅らせる（[`EffectPlugins::discover`]）。chain 無しの要求はここを通らない。

use cmrt_clack_timeline::ProcessBlockTiming;
use cmrt_core::effect::EffectChain;
use cmrt_core::{effect_chain_spec_from_embedded_json, EffectPlugins, EFFECT_CHAIN_JSON_KEY};

use super::post_process::InstancePostProcess;

/// chain の JSON から、instance の後処理を作る。
///
/// bank worker のスレッドの上で呼ばれ、返した後処理もそのスレッドの上でだけ使う。
pub(super) trait LiveChainFactory: Send + Sync {
    /// `chain_json` は MML 先頭 JSON の `"effects after instrument"` の値。
    /// 有効な段が 1 つも無ければ `Ok(None)`。
    fn build(
        &self,
        chain_json: &str,
        sample_rate: f64,
        buf_size: usize,
    ) -> Result<Option<Box<dyn InstancePostProcess>>, String>;
}

/// 実 plugin の chain を作る factory。
pub(super) struct EffectPluginsChainFactory {
    plugins: EffectPlugins,
}

impl EffectPluginsChainFactory {
    pub(super) fn discover() -> Self {
        Self {
            plugins: EffectPlugins::discover(),
        }
    }
}

impl LiveChainFactory for EffectPluginsChainFactory {
    fn build(
        &self,
        chain_json: &str,
        sample_rate: f64,
        buf_size: usize,
    ) -> Result<Option<Box<dyn InstancePostProcess>>, String> {
        let chain: serde_json::Value = serde_json::from_str(chain_json)
            .map_err(|error| format!("effect chain の JSON が壊れている: {error}"))?;
        let embedded = serde_json::json!({ EFFECT_CHAIN_JSON_KEY: chain });
        self.plugins.with_render_effects(|effects| {
            let catalog = self
                .plugins
                .catalog()
                .ok_or_else(|| "effect plugin を使えない".to_string())?;
            let spec = effect_chain_spec_from_embedded_json(&embedded, catalog)
                .map_err(|error| format!("{error:#}"))?;
            if spec.is_empty() {
                return Ok(None);
            }
            let chain = effects
                .build_chain(&spec, sample_rate, buf_size)
                .and_then(|mut chain| chain.ensure_active().map(|()| chain))
                .map_err(|error| format!("effect chain {spec} の生成: {error:#}"))?;
            Ok(Some(
                Box::new(LiveEffectChain(chain)) as Box<dyn InstancePostProcess>
            ))
        })
    }
}

/// [`EffectChain`] を instance の後処理として通す。block の長さは render と同じ `buf_size`。
struct LiveEffectChain(EffectChain);

impl InstancePostProcess for LiveEffectChain {
    fn process(&mut self, samples: &mut [f32], timing: &ProcessBlockTiming) -> Result<(), String> {
        self.0
            .process_block(samples, timing.transport.as_ref())
            .map_err(|error| format!("effect chain: {error}"))
    }

    fn reset(&mut self) {
        self.0.reset();
    }
}
