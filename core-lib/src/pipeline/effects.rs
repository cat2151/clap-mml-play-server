//! offline render への effect chain の適用。
//!
//! chain は instrument の出力直後（WAV 書き出し前・preroll trim 前）に通す。
//! この位置は live 経路で bank worker の render 直後に入れるときと同じで、
//! 揃えておけば移行で音が変わらない。

use anyhow::{bail, Context, Result};
use clack_host::prelude::PluginEntry;
use cmrt_timeline::TempoMapTimeline;

use crate::audio_effect::{
    effect_chain_spec_from_embedded_json, embedded_json_has_effect_chain, AudioEffectCatalog,
    AudioEffectPluginInfo, EffectChainSpec, EFFECT_CHAIN_JSON_KEY,
};
use crate::effect::{EffectChain, EffectRenderer, EffectTransport};
use crate::CoreConfig;

/// effect plugin の entry を返す。呼び出し側が entry を保持し、render のたびに DLL を
/// 読み直さないようにする。
pub type EffectEntryLoader<'a> = &'a dyn Fn(&AudioEffectPluginInfo) -> Result<PluginEntry>;

/// render 経路が effect chain を扱えるかと、扱うための材料。
///
/// [`RenderEffects::unsupported`] の経路に chain 付きの MML が来たら、黙って dry で
/// 鳴らさずエラーにする。
#[derive(Clone, Copy)]
pub struct RenderEffects<'a> {
    support: Option<EffectSupport<'a>>,
}

#[derive(Clone, Copy)]
struct EffectSupport<'a> {
    catalog: &'a AudioEffectCatalog,
    load_entry: EffectEntryLoader<'a>,
}

impl<'a> RenderEffects<'a> {
    pub fn new(catalog: &'a AudioEffectCatalog, load_entry: EffectEntryLoader<'a>) -> Self {
        Self {
            support: Some(EffectSupport {
                catalog,
                load_entry,
            }),
        }
    }

    /// chain 付きの MML を受け付けない経路。
    pub fn unsupported() -> Self {
        Self { support: None }
    }

    /// MML 先頭 JSON から chain を引く。JSON が無い・キーが無いなら空。
    pub(super) fn chain_spec(&self, embedded_json: Option<&str>) -> Result<EffectChainSpec> {
        let Some(json) = embedded_json else {
            return Ok(EffectChainSpec::default());
        };
        let Some(support) = self.support else {
            if embedded_json_has_effect_chain(Some(json)) {
                bail!("この render 経路は '{EFFECT_CHAIN_JSON_KEY}' に対応していない");
            }
            return Ok(EffectChainSpec::default());
        };
        let value: serde_json::Value = match serde_json::from_str(json) {
            Ok(value) => value,
            // JSON が壊れているときの扱いは音色の解決（`extract_patch_from_json`）と同じで、
            // 指定が無かったものとして進める。
            Err(_) => return Ok(EffectChainSpec::default()),
        };
        effect_chain_spec_from_embedded_json(&value, support.catalog)
    }

    /// render 済みの interleaved stereo に chain を通す。空なら何もしない。
    pub(super) fn apply(
        &self,
        spec: &EffectChainSpec,
        samples: &mut Vec<f32>,
        cfg: &CoreConfig,
        tempo_map: Option<&TempoMapTimeline>,
        musical_origin_samples: u64,
    ) -> Result<()> {
        if spec.is_empty() {
            return Ok(());
        }
        let mut chain = self.build_chain(spec, cfg.sample_rate, cfg.buffer_size)?;
        chain
            .process_all(
                samples,
                EffectTransport {
                    tempo_map,
                    musical_origin_samples,
                },
            )
            .with_context(|| format!("effect chain {spec} の適用"))
    }

    /// 返した chain を通す block の長さは `buf_size` ちょうどにすること（[`EffectChain`] の制約）。
    pub fn build_chain(
        &self,
        spec: &EffectChainSpec,
        sample_rate: f64,
        buf_size: usize,
    ) -> Result<EffectChain> {
        let Some(support) = self.support else {
            bail!("この render 経路は '{EFFECT_CHAIN_JSON_KEY}' に対応していない");
        };
        let mut stages = Vec::with_capacity(spec.len());
        for stage in spec.stages() {
            let plugin = support.catalog.plugin(&stage.plugin)?;
            let entry = (support.load_entry)(plugin)
                .with_context(|| format!("effect plugin '{}' のロード", plugin.name))?;
            let mut renderer =
                EffectRenderer::new(&entry, &plugin.plugin_id, sample_rate, buf_size)
                    .with_context(|| format!("effect '{}' の生成", plugin.name))?;
            renderer
                .load_preset_file(&stage.preset.path)
                .with_context(|| format!("effect preset '{}' の適用", stage.preset.display))?;
            stages.push(renderer);
        }
        EffectChain::new(stages)
    }
}
