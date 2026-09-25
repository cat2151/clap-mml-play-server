//! 音色の準備に同梱された effect chain を、bank の instance ごとの後処理として持つ。
//!
//! 音色と chain は同じ準備の要求で差し替える。音色が今載っているものと同じで chain だけが
//! 違う要求では、instrument を読み直さず chain だけ作り直す。chain を作れなければ音色も
//! 載せずに準備を失敗させる。

use std::{sync::Arc, time::Instant};

use cmrt_clack_timeline::ProcessBlockTiming;

use super::live_chain::LiveChainFactory;
use super::post_process::{InstancePostProcess, InstancePostProcesses};
use super::protocol::{PatchJob, PatchOutcome, RenderedSamples};

/// 1 instance に今載っている音色と chain。
#[derive(Default)]
struct LoadedSlot {
    /// 最後に準備が成功した音色。`None` は「分からない」（起動直後・probe の後・失敗の後）。
    patch: Option<Option<String>>,
    /// 今掛かっている chain の JSON。空は chain 無し。
    chain: String,
}

/// bank worker へ渡す、chain を作る材料。bank ごとに複製して渡す。
#[derive(Clone)]
pub(super) struct ChainSource {
    pub(super) factory: Arc<dyn LiveChainFactory>,
    pub(super) sample_rate: f64,
}

pub(super) struct InstanceChains {
    bank: usize,
    source: ChainSource,
    buf_size: usize,
    post_processes: InstancePostProcesses,
    loaded: Vec<LoadedSlot>,
}

impl InstanceChains {
    pub(super) fn new(
        bank: usize,
        source: ChainSource,
        buf_size: usize,
        instance_count: usize,
    ) -> Self {
        Self {
            bank,
            source,
            buf_size,
            post_processes: InstancePostProcesses::empty(instance_count),
            loaded: (0..instance_count).map(|_| LoadedSlot::default()).collect(),
        }
    }

    /// 音色と chain を準備する。`load_patch` は instrument の音色の差し替え
    /// （`switch_patch`）で、chain だけが変わる要求では呼ばない。
    pub(super) fn prepare(
        &mut self,
        job: &PatchJob,
        load_patch: impl FnOnce() -> PatchOutcome<()>,
    ) -> PatchOutcome<()> {
        let local_index = job.local_index;
        let Some(slot) = self.loaded.get(local_index) else {
            return load_patch();
        };
        let chain_changed = slot.chain != job.effect_chain;
        let same_patch = slot.patch.as_ref() == Some(&job.patch);
        let started = Instant::now();
        let new_chain = if chain_changed {
            match self.build_chain(&job.effect_chain) {
                Ok(chain) => chain,
                Err(error) => {
                    self.log_chain(local_index, "build-failed", false, started);
                    return PatchOutcome {
                        swapped: false,
                        result: Err(error),
                    };
                }
            }
        } else {
            None
        };
        let skip_patch = chain_changed && same_patch;
        let outcome = if skip_patch {
            PatchOutcome {
                swapped: false,
                result: Ok(()),
            }
        } else {
            load_patch()
        };
        let slot = &mut self.loaded[local_index];
        if outcome.result.is_err() {
            slot.patch = None;
            return outcome;
        }
        slot.patch = Some(job.patch.clone());
        if chain_changed {
            slot.chain.clone_from(&job.effect_chain);
            let event = if new_chain.is_some() {
                "rebuilt"
            } else {
                "removed"
            };
            self.post_processes.set(local_index, new_chain);
            self.log_chain(local_index, event, skip_patch, started);
        }
        outcome
    }

    /// instance の音色が準備以外の経路（voicing probe）で変わった。次の準備では必ず読み直す。
    pub(super) fn forget_patch(&mut self, local_index: usize) {
        if let Some(slot) = self.loaded.get_mut(local_index) {
            slot.patch = None;
        }
    }

    pub(super) fn apply(
        &mut self,
        local_index: usize,
        rendered: RenderedSamples,
        timing: &ProcessBlockTiming,
    ) -> RenderedSamples {
        self.post_processes.apply(local_index, rendered, timing)
    }

    pub(super) fn reset(&mut self, local_index: usize) {
        self.post_processes.reset(local_index);
    }

    pub(super) fn reset_all(&mut self) {
        for local_index in 0..self.loaded.len() {
            self.post_processes.reset(local_index);
        }
    }

    fn build_chain(
        &self,
        chain_json: &str,
    ) -> Result<Option<Box<dyn InstancePostProcess>>, String> {
        if chain_json.is_empty() {
            return Ok(None);
        }
        self.source
            .factory
            .build(chain_json, self.source.sample_rate, self.buf_size)
    }

    fn log_chain(&self, local_index: usize, event: &str, patch_skipped: bool, started: Instant) {
        eprintln!(
            "cmrt-bank-chain: bank={} local={local_index} event={event} patch_skipped={patch_skipped} \
             elapsed_ms={}",
            self.bank,
            started.elapsed().as_millis(),
        );
    }
}

#[cfg(test)]
mod tests;
