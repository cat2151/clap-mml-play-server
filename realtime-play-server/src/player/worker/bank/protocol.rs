//! coordinator と bank worker のあいだで行き来する仕事と返事。
//!
//! 責務ごとに 1 つの [`BankCommand`] にしてある。要求は常に 1 つずつで、
//! 返事は 1 本の channel に相乗りさせる（`BankReply` の変種が要求と 1 対 1）。

use cmrt_core::{LiveMidiEvent, RealtimePlaybackSchedule, VoicingReport};

use cmrt_clack_timeline::ProcessBlockTiming;

/// bank worker へ渡す仕事。
pub(super) enum BankCommand {
    /// この bank の instance を local index 順に 1 ブロック render する。
    RenderBlock {
        timing: ProcessBlockTiming,
        instances: Vec<BankRenderInstance>,
    },
    /// HTTP / MML の scheduled 再生。bank 0 の instance 0 だけが使う。
    ///
    /// 責務としては `RenderBlock` と別物（イベント列ではなく再生スケジュールが進む）。
    /// スケジュールの所有権は coordinator に残したいので、1 ブロックごとに往復させる
    /// （`Vec` の移動だけで、コピーは起きない）。
    RenderScheduled(Box<RealtimePlaybackSchedule>),
    PreparePatch(PatchJob),
    ProbePatch(PatchJob),
    ResetInstance {
        local_index: usize,
    },
    ResetAll,
    Shutdown,
}

/// 1 instance ぶんの render 要求。
pub(super) struct BankRenderInstance {
    pub(super) local_index: usize,
    pub(super) events: Vec<LiveMidiEvent>,
}

/// 音色差し替えの仕事。
///
/// **物理インスタンスは載っていない。** プラグイン種別が変わる差し替えは、
/// その bank worker が自分の予備プール（[`super::super::super::instances::LiveInstances`]）
/// から取り出して自分で入れ替える（Stage 4）。
pub(super) struct PatchJob {
    pub(super) local_index: usize,
    pub(super) patch: Option<String>,
    /// `set_patch` の前に鳴っている音を止めるか。
    pub(super) reset_before: bool,
    /// `set_patch` のあと反映のために空回しするか。
    pub(super) settle: bool,
}

/// bank worker からの返事。
pub(super) enum BankReply {
    Rendered(Vec<BankRendered>),
    Scheduled {
        playback: Box<RealtimePlaybackSchedule>,
        result: Result<Option<Vec<f32>>, String>,
    },
    Patched(PatchOutcome<()>),
    Probed(PatchOutcome<VoicingReport>),
}

/// 1 instance ぶんの render 結果。失敗した instance だけが `Err` になる。
pub(super) type RenderedSamples = Result<Vec<f32>, String>;

/// global instance index 順に並べた render 結果。要求しなかった instance は `None`。
pub(super) type RenderedInstances = Vec<Option<RenderedSamples>>;

pub(super) struct BankRendered {
    pub(super) local_index: usize,
    pub(super) samples: RenderedSamples,
}

/// 音色差し替えの結果。**押し出された物理インスタンスは返らない**
/// （所有 bank の予備プールへその場で戻っている）。
pub(super) struct PatchOutcome<T> {
    /// プラグイン種別の差し替えが起きたか。ログの `swapped=` に出す。
    pub(super) swapped: bool,
    pub(super) result: Result<T, String>,
}
