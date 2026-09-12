//! bank ごとの worker スレッドと、そこへ渡すコマンド。
//!
//! # なぜ分けるか
//! 従来は 1 本の player worker がコマンド処理・全 instance の直列 render・patch load を
//! すべて担っていた。そのため、鳴っていない待機 bank へ音色を先読みしているだけでも
//! `set_patch` と settle の間ずっと演奏 bank の render が止まる。
//!
//! bank ごとに固定の worker スレッドがその bank の物理インスタンスを所有し続け、
//! coordinator（[`super::run_player_worker`]）は CLAP の `process()` も `set_patch()` も
//! 直接呼ばず、[`BankCommand`] を投げるだけになる。
//!
//! # 先読み中に演奏 bank が止まらない仕組み
//! coordinator は先読み（[`BankWorkers::start_patch`]）を**送るだけで戻る**。
//! 返事を受け取るまでその bank は「塞がっている」ので render 対象から外れ
//! （[`BankWorkers::render_enabled_for`]）、演奏 bank だけが毎ブロック回り続ける。
//! 返事は毎周回 [`BankWorkers::try_finish_patch`] で拾う。
//! render も 1 bank ずつ待たず、両 bank へ送ってから返事を集める。
//!
//! **render 可否は「返事待ちの仕事があるか」1 つから決まる。** 別に真偽値を持って
//! 「先読み中」を表すと、消し忘れたときにその bank が二度と鳴らなくなる。
//!
//! # 予備インスタンスの袋も bank worker が持つ
//! 「論理スロット → 物理インスタンスの種別」の台帳と予備の袋
//! （[`super::super::instances::LiveInstances`]）は bank ごとに 1 つで、所有 bank の
//! worker スレッドの上で作られる。coordinator は袋に触れないので、プラグイン種別が
//! 変わる差し替えでも `RendererHandoff` の往復
//! （「袋から出す → worker へ送る → 押し出された方を受け取って袋へ戻す」）が要らない。
//! 予備の目標数は 2 bank の**合計が分離前と同じ**になるように割ってある。
//!
//! # renderer をどう配るか
//! 起動時のインスタンス生成は従来どおり [`super::super::startup::create_live_renderers`]
//! が 1 回で全部作る。bank ごとに作り直すと `load_entry` が 2 回走り、並列生成の
//! スレッド数も半分ずつに割れて起動時間が延びる（起動 8.8 秒 → 0.9 秒の最適化を削る）。
//! 作った renderer は `RendererHandoff` で bank worker へ渡す。これは生成用スレッドから
//! player worker へ渡していた従来と同じ 1 回の移送で、`docs/adr/0009-unsafe-thread-handoff.md`
//! の賭けを新しく増やしてはいない。**通常の bank 切替では renderer を動かさない。**

mod handle;
mod patch;
mod protocol;
mod state;

use cmrt_clack_timeline::ProcessBlockTiming;
use cmrt_core::{LiveMidiEvent, RealtimePlaybackSchedule, RealtimeRenderer};

use self::handle::BankWorker;
use self::protocol::{BankCommand, BankRenderInstance, BankReply, RenderedInstances};
use super::super::bank::{BankLayout, BankSlot, BANK_COUNT};
use super::super::instances::{plan_bank_instances, PluginKind};

pub(super) use self::patch::PendingPatch;

/// 2 つの bank worker と、instance ID をそこへ割り振る規則。
pub(super) struct BankWorkers {
    layout: BankLayout,
    workers: Vec<BankWorker>,
    buf_size: usize,
}

impl BankWorkers {
    /// 起動時に作った renderer 群を bank へ配り、bank ごとの worker を起こす。
    ///
    /// 前半（端数は bank 0 側）が bank 0、後半が bank 1。クライアント側の割り当て
    /// （grid sequencer の `state/cycle.rs`）と同じ規則である[`BankLayout`]に従う。
    ///
    /// `kinds` は予備プールの材料。袋そのものは各 worker が自分のスレッドで作る
    /// （`!Send` な物理インスタンスを持つため）。
    pub(super) fn start(renderers: Vec<RealtimeRenderer>, kinds: Vec<PluginKind>) -> Self {
        let layout = BankLayout::split_any(renderers.len());
        let buf_size = renderers.first().map_or(0, RealtimeRenderer::buf_size);
        let mut first = renderers;
        let second = first.split_off(layout.bank_size(0));
        let mut specs =
            plan_bank_instances(kinds, [layout.bank_size(0), layout.bank_size(1)]).into_iter();
        // bank ごとに直列に起こす。プラグインによっては複数インスタンスの同時生成で
        // 落ちる（Vaporizer2）ので、生成そのものは呼び出し側が 1 回で済ませ、
        // ここでは出来上がったものを配るだけにしてある。
        let workers = vec![
            BankWorker::spawn(0, first, specs.next().expect("bank の数だけ作ってある")),
            BankWorker::spawn(1, second, specs.next().expect("bank の数だけ作ってある")),
        ];
        Self {
            layout,
            workers,
            buf_size,
        }
    }

    pub(super) fn instance_count(&self) -> usize {
        self.layout.instance_count()
    }

    /// 1 ブロックのフレーム数。全 instance で同じ。
    pub(super) fn buf_size(&self) -> usize {
        self.buf_size
    }

    /// この instance を所有している bank。
    pub(super) fn bank_of(&self, instance_index: usize) -> usize {
        self.layout.slot_of_index(instance_index).bank
    }

    /// この instance を render してよいか。
    ///
    /// **音色ロードの返事待ちの bank は `false`。** その worker はロードで塞がっていて、
    /// render を送ってもロードの後ろに並ぶだけ（＝演奏が止まる）。ロードの返事を
    /// 引き取った時点で自動的に `true` へ戻るので、消し忘れで鳴らなくなることがない。
    pub(super) fn render_enabled_for(&self, instance_index: usize) -> bool {
        !self.workers[self.bank_of(instance_index)].busy()
    }

    /// 塞がっているせいで render を出せなかった instance を 1 つ数える。
    ///
    /// 正規の grid 経路では、先読み中の bank 宛に鳴らすものは無い（クライアントが
    /// 「発音 deadline を越えた待機 bank だ」と宣言して送っている）。**増えていたら
    /// その契約が破れている**ので、先読みの完了行へ出して検出できるようにしてある。
    pub(super) fn note_render_skip(&self, instance_index: usize) {
        self.workers[self.bank_of(instance_index)].note_render_skip();
    }

    /// この bank で render を出せなかった instance の延べ数。
    pub(super) fn render_skips(&self, bank: usize) -> u64 {
        self.workers[bank].render_skips()
    }

    /// 指定した bank 以外が render したブロック数の合計。
    ///
    /// 先読みの前後で差を取れば「ロード中に演奏 bank が何ブロック進んだか」が出る。
    pub(super) fn blocks_rendered_elsewhere(&self, bank: usize) -> u64 {
        self.workers
            .iter()
            .filter(|worker| worker.bank() != bank)
            .map(BankWorker::blocks)
            .sum()
    }

    /// 全 instance の音を止める。返事は待たない（順序は channel が保つ）。
    pub(super) fn reset_all(&self) {
        for worker in &self.workers {
            worker.notify(BankCommand::ResetAll);
        }
    }

    /// 1 instance の音を止める。
    pub(super) fn reset_instance(&self, instance_index: usize) {
        let slot = self.layout.slot_of_index(instance_index);
        self.workers[slot.bank].notify(BankCommand::ResetInstance {
            local_index: slot.local_index,
        });
    }

    /// global instance index 順に並べた「その instance のイベント」を bank へ配って
    /// render し、結果を global instance index 順へ戻す。
    ///
    /// `None` の要素は render を要求しない instance（非 active、または先読み中の bank）。
    /// 返る `Vec` も同じ長さで、要求しなかった instance は `None`。
    ///
    /// **両 bank へ送ってから返事を集める。** 1 bank ずつ送って待つと、2 本の worker が
    /// 交互に動くだけで並行にならない。合成順（= 浮動小数の加算順）は呼び出し側が
    /// global instance index 順に戻すので、送信順にも到着順にも依存しない。
    pub(super) fn render_instances(
        &self,
        per_instance: Vec<Option<Vec<LiveMidiEvent>>>,
        timing: ProcessBlockTiming,
    ) -> anyhow::Result<RenderedInstances> {
        let jobs = split_events_by_bank(&self.layout, per_instance);
        let mut requested = [false; BANK_COUNT];
        let mut error = None;
        for (bank, instances) in jobs.into_iter().enumerate() {
            if instances.is_empty() || error.is_some() {
                continue;
            }
            if self.workers[bank].busy() {
                // coordinator 側で先読み中の bank を除いているので通常は来ない。
                eprintln!("cmrt-bank-render: bank={bank} event=skipped reason=busy");
                continue;
            }
            match self.workers[bank].send(BankCommand::RenderBlock { timing, instances }) {
                Ok(()) => requested[bank] = true,
                Err(send_error) => error = Some(send_error),
            }
        }
        // 送った bank の返事は必ず引き取る。引き取らないと channel の対応がずれる。
        let mut rendered = vec![None; self.layout.instance_count()];
        for (bank, requested) in requested.into_iter().enumerate() {
            if !requested {
                continue;
            }
            match self.workers[bank].receive() {
                Ok(BankReply::Rendered(results)) => {
                    for result in results {
                        let index = self.layout.global_index(BankSlot {
                            bank,
                            local_index: result.local_index,
                        });
                        rendered[index] = Some(result.samples);
                    }
                }
                Ok(_) => {
                    error = Some(anyhow::anyhow!(
                        "bank {bank} worker returned an unexpected reply"
                    ));
                }
                Err(receive_error) => error = Some(receive_error),
            }
        }
        match error {
            Some(error) => Err(error),
            None => Ok(rendered),
        }
    }

    /// scheduled 再生を 1 ブロック進める。`Ok(None)` は再生終了。
    pub(super) fn render_scheduled(
        &self,
        playback: &mut RealtimePlaybackSchedule,
    ) -> anyhow::Result<Option<Vec<f32>>> {
        // worker へ移すため一旦取り出す。失敗した場合は呼び出し側が再生を畳むので、
        // 置き去りになる空スケジュールは使われない。
        let taken = std::mem::replace(playback, RealtimePlaybackSchedule::new(Vec::new(), 0));
        let reply = self.workers[0].request(BankCommand::RenderScheduled(Box::new(taken)))?;
        let BankReply::Scheduled {
            playback: returned,
            result,
        } = reply
        else {
            anyhow::bail!("bank 0 worker returned an unexpected reply");
        };
        *playback = *returned;
        result.map_err(|error| anyhow::anyhow!(error))
    }

    /// 両 bank へ `Shutdown` を送り、join する。
    pub(super) fn shutdown(&mut self) {
        for worker in &self.workers {
            worker.notify(BankCommand::Shutdown);
        }
        for worker in &mut self.workers {
            if let Some(join) = worker.take_join() {
                let _ = join.join();
            }
        }
    }
}

impl Drop for BankWorkers {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// global instance index 順のイベント列を bank ごとの render 要求へ割る。
fn split_events_by_bank(
    layout: &BankLayout,
    per_instance: Vec<Option<Vec<LiveMidiEvent>>>,
) -> [Vec<BankRenderInstance>; BANK_COUNT] {
    let mut jobs: [Vec<BankRenderInstance>; BANK_COUNT] = [Vec::new(), Vec::new()];
    for (index, events) in per_instance.into_iter().enumerate() {
        let Some(events) = events else {
            continue;
        };
        let slot = layout.slot_of_index(index);
        jobs[slot.bank].push(BankRenderInstance {
            local_index: slot.local_index,
            events,
        });
    }
    jobs
}

#[cfg(test)]
mod tests;
