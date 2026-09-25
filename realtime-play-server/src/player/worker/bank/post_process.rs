//! instance ごとの後処理（instance の render の直後、auto gain と mix の前）。
//!
//! 置き場は bank の中の local index ごとに 1 つ。何も入っていない instance は
//! render 結果をそのまま返す。後処理は bank worker のスレッドの上で render と直列に
//! 呼ばれるので、`!Send` な中身（CLAP の effect instance など）も持てる。

use cmrt_clack_timeline::ProcessBlockTiming;

use super::protocol::RenderedSamples;

/// 1 instance の render 結果に掛ける後処理。
///
/// `samples` は render と同じ interleaved stereo の 1 block。長さを変えてはいけない。
pub(super) trait InstancePostProcess {
    fn process(&mut self, samples: &mut [f32], timing: &ProcessBlockTiming) -> Result<(), String>;

    /// 残っている余韻・内部状態を捨てる（instance の renderer を `reset` するときに一緒に呼ぶ）。
    fn reset(&mut self) {}
}

/// bank の instance ごとの後処理の置き場。
pub(super) struct InstancePostProcesses {
    slots: Vec<Option<Box<dyn InstancePostProcess>>>,
}

impl InstancePostProcesses {
    /// `instance_count` 個の空の置き場。
    pub(super) fn empty(instance_count: usize) -> Self {
        Self {
            slots: (0..instance_count).map(|_| None).collect(),
        }
    }

    /// `local_index` の後処理を差し替える（`None` で外す）。
    pub(super) fn set(
        &mut self,
        local_index: usize,
        post_process: Option<Box<dyn InstancePostProcess>>,
    ) {
        if let Some(slot) = self.slots.get_mut(local_index) {
            *slot = post_process;
        }
    }

    /// `local_index` の後処理の状態を捨てる。
    pub(super) fn reset(&mut self, local_index: usize) {
        if let Some(Some(post_process)) = self.slots.get_mut(local_index) {
            post_process.reset();
        }
    }

    /// `local_index` の render 結果に後処理を通す。後処理の失敗は render の失敗と同じく `Err` になる。
    pub(super) fn apply(
        &mut self,
        local_index: usize,
        rendered: RenderedSamples,
        timing: &ProcessBlockTiming,
    ) -> RenderedSamples {
        let mut samples = rendered?;
        if let Some(Some(post_process)) = self.slots.get_mut(local_index) {
            post_process.process(&mut samples, timing)?;
        }
        Ok(samples)
    }
}

#[cfg(test)]
mod tests;
