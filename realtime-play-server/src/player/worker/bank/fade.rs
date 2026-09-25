//! instance ごとの fadeout（effect chain の後、auto gain と mix の前）。
//!
//! 要求を受けた instance の出力に、今の gain から 0 まで sample 単位の直線の ramp を掛ける。
//! 0 に達した block の後、その instance の renderer と chain を reset し、出力を 0 に保つ。
//!
//! 0 に保つのは、chain の `reset` が余韻を消さない effect があるため（Surge XT Effects の
//! reverb は CLAP の reset でも deactivate / activate でも余韻が残る）。0 を解くのは、
//! 新しい行の開始（timeline の張り直しか、その instance の音色の準備）の**後に**その
//! instance へイベントが届いたときで、そこから等倍で鳴る。新しい行の開始より前に届いた
//! イベントは、絞った行の続きなので捨てる。
//!
//! fade 中に届いた新しい行のイベントは renderer へ渡さずに預かり、0 に達した後の最初の
//! block の頭へまとめて置く。fade 中に鳴らすと新しい音の頭が絞られたうえ、reset で消えるため。

use cmrt_clack_timeline::ProcessBlockTiming;
use cmrt_core::LiveMidiEvent;

use super::instance_chain::InstanceChains;
use super::protocol::RenderedSamples;

/// fade 中に預かるイベントの初期容量。render の途中で確保しないよう先に取っておく。
const HELD_EVENTS_CAPACITY: usize = 256;

/// 今の gain から 0 へ向かう直線の ramp。
struct Ramp {
    gain: f32,
    step: f32,
    remaining_frames: u32,
}

impl Ramp {
    /// 1 frame 進めて、その frame に掛ける gain を返す。最後の frame で必ず 0 になる。
    fn next_gain(&mut self) -> f32 {
        if self.remaining_frames > 0 {
            self.remaining_frames -= 1;
            self.gain = if self.remaining_frames == 0 {
                0.0
            } else {
                (self.gain - self.step).max(0.0)
            };
        }
        self.gain
    }
}

struct FadeSlot {
    ramp: Option<Ramp>,
    /// 0 に達した後、出力を 0 に保っている。
    silenced: bool,
    /// fade の後に新しい行が始まった。以後のイベントは新しい行のもの。
    new_line: bool,
    /// fade 中に届いた新しい行のイベント。
    held: Vec<LiveMidiEvent>,
    /// `held` を前の block で renderer へ渡し終えた（次に触る前に空にする）。
    released: bool,
}

impl FadeSlot {
    fn new() -> Self {
        Self {
            ramp: None,
            silenced: false,
            new_line: false,
            held: Vec::with_capacity(HELD_EVENTS_CAPACITY),
            released: false,
        }
    }

    fn drop_released(&mut self) {
        if self.released {
            self.held.clear();
            self.released = false;
        }
    }

    fn clear(&mut self) {
        self.ramp = None;
        self.silenced = false;
        self.new_line = false;
        self.held.clear();
        self.released = false;
    }
}

/// bank の instance ごとの fadeout の状態。
pub(super) struct InstanceFades {
    slots: Vec<FadeSlot>,
}

impl InstanceFades {
    pub(super) fn new(instance_count: usize) -> Self {
        Self {
            slots: (0..instance_count).map(|_| FadeSlot::new()).collect(),
        }
    }

    /// `local_index` の出力を、今の gain から 0 まで `fade_frames` で絞り始める。
    /// fade 中に重ねて呼ぶと、その時点の gain から新しい長さで絞り直す。
    pub(super) fn start(&mut self, local_index: usize, fade_frames: u32) {
        let Some(slot) = self.slots.get_mut(local_index) else {
            return;
        };
        slot.drop_released();
        slot.held.clear();
        slot.new_line = false;
        if slot.silenced {
            return;
        }
        let gain = slot.ramp.as_ref().map_or(1.0, |ramp| ramp.gain);
        let frames = fade_frames.max(1);
        slot.ramp = Some(Ramp {
            gain,
            step: gain / frames as f32,
            remaining_frames: frames,
        });
    }

    /// 新しい行が始まった（timeline の張り直し）。絞った instance は、次に届くイベントから鳴らす。
    pub(super) fn begin_new_line_all(&mut self) {
        for slot in &mut self.slots {
            slot.new_line = true;
        }
    }

    /// `local_index` に新しい行を鳴らす準備が済んだ（音色の準備）。
    pub(super) fn begin_new_line(&mut self, local_index: usize) {
        if let Some(slot) = self.slots.get_mut(local_index) {
            slot.new_line = true;
        }
    }

    /// fade・0 の保持・預かったイベントを捨てる（instance ごと止めるとき）。
    pub(super) fn clear(&mut self, local_index: usize) {
        if let Some(slot) = self.slots.get_mut(local_index) {
            slot.clear();
        }
    }

    pub(super) fn clear_all(&mut self) {
        self.slots.iter_mut().for_each(FadeSlot::clear);
    }

    /// この block で renderer へ渡すイベント。
    ///
    /// fade 中と 0 の保持中は空を返す。新しい行が始まる前のイベントは捨て、始まった後の
    /// ものは預かる。0 の保持中に新しい行のイベントが届いたら保持を解き、預かったイベントを
    /// block の頭（offset 0）に置いて、その後ろへ `events` を並べて返す。
    pub(super) fn block_events<'a>(
        &'a mut self,
        local_index: usize,
        events: &'a [LiveMidiEvent],
    ) -> &'a [LiveMidiEvent] {
        let Some(slot) = self.slots.get_mut(local_index) else {
            return events;
        };
        slot.drop_released();
        if slot.ramp.is_none() && !slot.silenced {
            return events;
        }
        if !slot.new_line {
            return &[];
        }
        if slot.ramp.is_some() || (events.is_empty() && slot.held.is_empty()) {
            slot.held.extend_from_slice(events);
            return &[];
        }
        slot.silenced = false;
        slot.new_line = false;
        for event in &mut slot.held {
            event.offset_frames = 0;
        }
        slot.held.extend_from_slice(events);
        slot.released = true;
        &slot.held
    }

    /// interleaved stereo の 1 block に ramp（0 の保持中は 0）を掛ける。
    /// この block で 0 に達したら `true`。
    pub(super) fn apply(&mut self, local_index: usize, samples: &mut [f32]) -> bool {
        let Some(slot) = self.slots.get_mut(local_index) else {
            return false;
        };
        if slot.silenced {
            samples.fill(0.0);
            return false;
        }
        let Some(ramp) = slot.ramp.as_mut() else {
            return false;
        };
        for frame in samples.as_chunks_mut::<2>().0 {
            let gain = ramp.next_gain();
            frame[0] *= gain;
            frame[1] *= gain;
        }
        if ramp.remaining_frames > 0 {
            return false;
        }
        slot.ramp = None;
        slot.silenced = true;
        true
    }
}

/// 1 instance の render 結果に effect chain と fadeout を順に通す。
///
/// fade が 0 に達した block と、render・chain が失敗した block では、chain の状態を捨てて
/// `reset_voices`（instance の renderer の reset）を呼ぶ。
pub(super) fn finish_instance_output(
    chains: &mut InstanceChains,
    fades: &mut InstanceFades,
    local_index: usize,
    rendered: RenderedSamples,
    timing: &ProcessBlockTiming,
    reset_voices: impl FnOnce(),
) -> RenderedSamples {
    let mut output = chains.apply(local_index, rendered, timing);
    let reset = match output.as_mut() {
        Ok(samples) => fades.apply(local_index, samples),
        Err(_) => {
            // 壊れた instance だけを止める。同じ bank の他の instance は鳴り続ける。
            fades.clear(local_index);
            true
        }
    };
    if reset {
        reset_voices();
        chains.reset(local_index);
    }
    output
}

#[cfg(test)]
mod tests;
