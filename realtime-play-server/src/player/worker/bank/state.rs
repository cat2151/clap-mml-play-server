//! bank worker スレッド本体。
//!
//! **この bank の renderer 以外は持たない。** CLAP の `process()` と `set_patch()` を
//! 呼ぶのはここだけで、同じ instance に対する render と patch load はこの 1 本の
//! スレッド上で必ず直列に並ぶ（CLAP の thread 制約を型の形で満たすための構造）。
//!
//! Stage 4 で予備インスタンスの袋（[`LiveInstances`]）もここが持つようになった。
//! プラグイン種別が変わる差し替えは「袋から出す → 載っているものと入れ替える →
//! 押し出されたものを袋へ戻す」まで**このスレッドの中で完結する**。袋が尽きて
//! 背景生成を待つときも、待つのはこのスレッドだけで反対の bank は render を続ける。

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, Sender},
        Arc,
    },
    time::{Duration, Instant},
};

use cmrt_clack_timeline::ProcessBlockTiming;
use cmrt_core::{RealtimePlaybackSchedule, RealtimeRenderer, RendererHandoff, VoicingReport};

use super::super::super::instances::{LiveInstances, LiveInstancesSpec};
use super::protocol::{
    BankCommand, BankRenderInstance, BankRendered, BankReply, PatchJob, PatchOutcome,
};

/// `set_patch` のあと空回しするブロック数。プラグインによっては state load の反映に
/// `process()` が要る（Dexed）。
///
/// **空回しは鳴っている音の再生位置を進める。** 鳴っている voice を跨いで差し替える
/// プラグイン（cache-player）では 1 ブロックも回さない。その判断は
/// `RealtimeRenderer::switch_patch` の中にあり、ここは「回してよいなら何ブロックか」
/// だけを決める。
const PATCH_SETTLE_BLOCKS: usize = 4;

/// render している thread をログへ出す間隔（block 数）。
///
/// 毎ブロック出すと 48kHz / 512 frame で毎秒 94 行になる。**1 ブロック目は必ず出す**ので、
/// 「どの thread が render しているか」は演奏開始直後の 1 行で分かる。
const RENDER_LOG_INTERVAL_BLOCKS: u64 = 4096;

/// 音色ロードを人工的に遅らせる**テスト専用**の環境変数（ミリ秒）。
///
/// 「先読み中も演奏 bank が回り続ける」（受け入れ条件 2）は、ロードが十分長いときにしか
/// 差が出ない。実プラグインのロード時間はマシン依存で、テストから止めるのも不安定なので、
/// ここを唯一の注入点にしてある。設定しなければ 1 命令も増えない。
const PATCH_LOAD_DELAY_ENV: &str = "CMRT_TEST_PATCH_LOAD_DELAY_MS";

struct BankState {
    bank: usize,
    renderers: Vec<RealtimeRenderer>,
    /// この bank の「論理スロット → 物理インスタンスの種別」台帳と予備の袋。
    instances: LiveInstances,
    /// この bank が render したブロック数。**coordinator が演奏中に読む**ので共有する
    /// （先読み中に演奏 bank が何ブロック進んだかを数える）。
    blocks: Arc<AtomicU64>,
    patch_load_delay: Duration,
}

pub(super) fn run_bank_worker(
    bank: usize,
    renderers: Vec<RendererHandoff>,
    spec: LiveInstancesSpec,
    blocks: Arc<AtomicU64>,
    commands: &Receiver<BankCommand>,
    replies: &Sender<BankReply>,
) {
    let mut state = BankState {
        bank,
        renderers: renderers
            .into_iter()
            .map(RendererHandoff::into_inner)
            .collect(),
        // 予備の袋は `!Send` な物理インスタンスを持つので、**このスレッドの上で作る**。
        instances: LiveInstances::new(spec),
        blocks,
        patch_load_delay: patch_load_delay(),
    };
    eprintln!(
        "cmrt-bank-worker: bank={bank} event=started thread={:?} instances={}",
        std::thread::current().id(),
        state.renderers.len(),
    );
    while let Ok(command) = commands.recv() {
        // 背景で出来上がった予備を袋へ取り込む。ブロックしない。
        state.instances.collect_ready();
        let reply = match command {
            BankCommand::Shutdown => break,
            BankCommand::ResetAll => {
                state.reset_all();
                continue;
            }
            BankCommand::ResetInstance { local_index } => {
                state.reset_instance(local_index);
                continue;
            }
            BankCommand::RenderBlock { timing, instances } => {
                BankReply::Rendered(state.render_block(timing, instances))
            }
            BankCommand::RenderScheduled(playback) => state.render_scheduled(playback),
            BankCommand::PreparePatch(job) => BankReply::Patched(state.prepare_patch(job)),
            BankCommand::ProbePatch(job) => BankReply::Probed(state.probe_patch(job)),
        };
        // 受け取り手が消えていたら、この bank にもう仕事は来ない。
        if replies.send(reply).is_err() {
            break;
        }
    }
    eprintln!(
        "cmrt-bank-worker: bank={bank} event=stopped thread={:?} blocks={}",
        std::thread::current().id(),
        state.blocks(),
    );
}

/// テスト用の人工ロード遅延。壊れた値は 0 として扱う（本番で誤設定されても素通し）。
fn patch_load_delay() -> Duration {
    std::env::var(PATCH_LOAD_DELAY_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(Duration::ZERO, Duration::from_millis)
}

impl BankState {
    fn reset_all(&mut self) {
        for renderer in &mut self.renderers {
            renderer.reset();
        }
    }

    fn reset_instance(&mut self, local_index: usize) {
        if let Some(renderer) = self.renderers.get_mut(local_index) {
            renderer.reset();
        }
    }

    fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }

    /// 1 ブロック進めたことを記録する。戻り値は通算ブロック数。
    fn count_block(&self) -> u64 {
        self.blocks.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn render_block(
        &mut self,
        timing: ProcessBlockTiming,
        instances: Vec<BankRenderInstance>,
    ) -> Vec<BankRendered> {
        let blocks = self.count_block();
        self.log_render_progress(blocks);
        let mut rendered = Vec::with_capacity(instances.len());
        for instance in instances {
            let renderer = &mut self.renderers[instance.local_index];
            let samples = match renderer.render_live_chunk_with_timing(&instance.events, timing) {
                Ok(samples) => Ok(samples),
                Err(error) => {
                    // 壊れた instance だけを止める。同じ bank の他の instance は鳴り続ける。
                    renderer.reset();
                    Err(format!("{error:#}"))
                }
            };
            rendered.push(BankRendered {
                local_index: instance.local_index,
                samples,
            });
        }
        rendered
    }

    fn render_scheduled(&mut self, mut playback: Box<RealtimePlaybackSchedule>) -> BankReply {
        let blocks = self.count_block();
        self.log_render_progress(blocks);
        let result = match self.renderers.first_mut() {
            Some(renderer) => renderer
                .render_next_chunk(&mut playback)
                .map_err(|error| format!("{error:#}")),
            None => Err("bank 0 has no live instance".to_string()),
        };
        BankReply::Scheduled { playback, result }
    }

    fn prepare_patch(&mut self, job: PatchJob) -> PatchOutcome<()> {
        let started = Instant::now();
        self.delay_for_test();
        let outcome = match self.swap_in(&job) {
            Ok(swapped) => {
                let renderer = &mut self.renderers[job.local_index];
                let settle_blocks = if job.settle { PATCH_SETTLE_BLOCKS } else { 0 };
                let result = renderer
                    .switch_patch(job.patch.as_deref(), job.reset_before, settle_blocks)
                    .map_err(|error| format!("{error:#}"));
                PatchOutcome { swapped, result }
            }
            Err(error) => PatchOutcome {
                swapped: false,
                result: Err(error),
            },
        };
        self.log_patch("prepare", job.local_index, started, &outcome);
        outcome
    }

    fn probe_patch(&mut self, job: PatchJob) -> PatchOutcome<VoicingReport> {
        let started = Instant::now();
        self.delay_for_test();
        let outcome = match self.swap_in(&job) {
            Ok(swapped) => {
                let renderer = &mut self.renderers[job.local_index];
                let result = renderer
                    .switch_patch(job.patch.as_deref(), job.reset_before, 0)
                    .and_then(|()| renderer.probe_voicing())
                    .map_err(|error| format!("{error:#}"));
                PatchOutcome { swapped, result }
            }
            Err(error) => PatchOutcome {
                swapped: false,
                result: Err(error),
            },
        };
        self.log_patch("probe", job.local_index, started, &outcome);
        outcome
    }

    /// 人工ロード遅延（[`PATCH_LOAD_DELAY_ENV`]）。**この bank worker の中でだけ眠る。**
    /// 分離できていれば、この間も反対の bank は render を続けられる。
    fn delay_for_test(&self) {
        if self.patch_load_delay.is_zero() {
            return;
        }
        eprintln!(
            "cmrt-bank-patch-delay: bank={} thread={:?} ms={}",
            self.bank,
            std::thread::current().id(),
            self.patch_load_delay.as_millis(),
        );
        std::thread::sleep(self.patch_load_delay);
    }

    /// 音色に要るプラグインをスロットへ用意する。戻り値は差し替えが起きたか。
    ///
    /// 袋は同じスレッドが持っているので、取り出しと戻しがここで一息に終わる。
    /// 尽きていれば背景生成を待つ（待つのはこのスレッドだけで、反対の bank は
    /// render を続けられる）。
    fn swap_in(&mut self, job: &PatchJob) -> Result<bool, String> {
        self.instances.prepare_slot_for_patch(
            &mut self.renderers,
            job.local_index,
            job.patch.as_deref(),
        )
    }

    /// **どの thread が render しているか**を機械可読に残す。patch load 側の
    /// `cmrt-bank-patch:` と thread ID を突き合わせれば、分離できているかが分かる。
    fn log_render_progress(&self, blocks: u64) {
        if blocks == 1 || blocks.is_multiple_of(RENDER_LOG_INTERVAL_BLOCKS) {
            eprintln!(
                "cmrt-bank-render: bank={} thread={:?} block={blocks}",
                self.bank,
                std::thread::current().id(),
            );
        }
    }

    fn log_patch<T>(
        &self,
        kind: &str,
        local_index: usize,
        started: Instant,
        outcome: &PatchOutcome<T>,
    ) {
        eprintln!(
            "cmrt-bank-patch: bank={} local={local_index} kind={kind} thread={:?} \
             swapped={} elapsed_ms={} result={}",
            self.bank,
            std::thread::current().id(),
            outcome.swapped,
            started.elapsed().as_millis(),
            if outcome.result.is_ok() {
                "ok"
            } else {
                "error"
            },
        );
    }
}
