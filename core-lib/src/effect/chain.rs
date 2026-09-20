//! 複数の [`EffectRenderer`] を直列に通す。

use anyhow::{bail, Result};
use clack_host::events::event_types::TransportEvent;
use cmrt_clack_timeline::process_block_timing;
use cmrt_timeline::{BlockSpan, FreeRunningTimeline, SamplePosition, TempoMapTimeline};

use super::renderer::{EffectProcessError, EffectRenderer};

/// [`EffectChain::process_all`] が block ごとの CLAP transport を作るための材料。
/// instrument の render に使った tempo map と preroll をそのまま渡す。
#[derive(Clone, Copy, Debug, Default)]
pub struct EffectTransport<'a> {
    /// `None` なら transport を渡さない（tempo-sync する delay は自由走行になる）。
    pub tempo_map: Option<&'a TempoMapTimeline>,
    /// 拍 0 に対応するサンプル位置（preroll のサンプル数）。
    pub musical_origin_samples: u64,
}

pub struct EffectChain {
    stages: Vec<EffectRenderer>,
}

impl EffectChain {
    /// 空でもよい。段ごとに `buf_size` / sample rate が違えばエラー。
    pub fn new(stages: Vec<EffectRenderer>) -> Result<Self> {
        if let Some((first, rest)) = stages.split_first() {
            for stage in rest {
                if stage.buf_size() != first.buf_size()
                    || stage.sample_rate() != first.sample_rate()
                {
                    bail!(
                        "effect chain の段で buf_size / sample rate が食い違う: '{}' と '{}'",
                        first.plugin_id(),
                        stage.plugin_id()
                    );
                }
            }
        }
        Ok(Self { stages })
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn stages_mut(&mut self) -> &mut [EffectRenderer] {
        &mut self.stages
    }

    /// 各段の `clap.latency` の合計（frame）。
    pub fn latency(&mut self) -> u32 {
        self.stages.iter_mut().map(EffectRenderer::latency).sum()
    }

    pub fn ensure_active(&mut self) -> Result<()> {
        self.stages
            .iter_mut()
            .try_for_each(EffectRenderer::ensure_active)
    }

    pub fn reset(&mut self) {
        self.stages.iter_mut().for_each(EffectRenderer::reset);
    }

    /// 1 block を全段に順に通す。RT で呼べる。
    pub fn process_block(
        &mut self,
        samples: &mut [f32],
        transport: Option<&TransportEvent>,
    ) -> Result<(), EffectProcessError> {
        self.stages
            .iter_mut()
            .try_for_each(|stage| stage.process(samples, transport))
    }

    /// render 済みの interleaved stereo 全体を通し、長さを元のまま返す。
    ///
    /// 合計 latency ぶん末尾に無音を足して回し、先頭の latency ぶんを捨てるので、
    /// 出力の位置は入力と揃う。Surge XT Effects は端数 block を 1 回でも受けると
    /// latency が変わる mode へ切り替わるので、`buf_size` の倍数まで無音で埋めて回す。
    pub fn process_all(
        &mut self,
        samples: &mut Vec<f32>,
        transport: EffectTransport,
    ) -> Result<()> {
        let Some(first) = self.stages.first() else {
            return Ok(());
        };
        if !samples.len().is_multiple_of(2) {
            bail!("interleaved stereo の長さが奇数: {}", samples.len());
        }
        let frames = samples.len() / 2;
        if frames == 0 {
            return Ok(());
        }
        let buf_size = first.buf_size();
        let sample_rate = first.sample_rate();
        self.ensure_active()?;
        let latency = self.latency() as usize;
        let padded_frames = (frames + latency).next_multiple_of(buf_size);
        samples.resize(padded_frames * 2, 0.0);
        for (block_index, block) in samples.chunks_exact_mut(buf_size * 2).enumerate() {
            let start = (block_index * buf_size) as u64;
            let musical = SamplePosition(start.saturating_sub(transport.musical_origin_samples));
            let span =
                BlockSpan::new(musical, buf_size as u32).map_err(|error| anyhow::anyhow!(error))?;
            let timing = match transport.tempo_map {
                Some(tempo_map) => process_block_timing(span, sample_rate, tempo_map),
                None => process_block_timing(span, sample_rate, &FreeRunningTimeline),
            };
            self.process_block(block, timing.transport.as_ref())?;
        }
        samples.drain(..latency * 2);
        samples.truncate(frames * 2);
        Ok(())
    }
}
