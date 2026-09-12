//! 論理 instance と物理 CLAP インスタンスの分離、および予備インスタンスプール。
//!
//! # なぜ要るか
//! `instance_id` はクライアント（TUI）が MIDI ルーティング・instance 別ゲイン・
//! auto gain の配列を引くための**論理スロット**で、1 プロセス 1 プラグインの前提が
//! 消えても意味を変えたくない。一方で「この行の音色は Dexed の cartridge」という要求は、
//! その行に載っている物理インスタンスを**別プラグインのものへ差し替える**ことを意味する。
//!
//! ここが持つのは「論理スロット → 物理インスタンス」の対応表と、プラグインごとの
//! 予備インスタンスの袋。差し替えは袋から取り出すだけなので即時に終わり、
//! 作り直し（Surge XT で 1 個 200〜360ms）を wheel の 1 目盛りごとに待たされない。
//!
//! # 誰が持つか
//! **bank worker が自分の bank ぶんを 1 つ持つ。** スロットは bank-local index で、
//! 袋の中身も所有 bank の物理インスタンスだけ。coordinator は台帳も袋も持たない。
//! そのおかげで差し替えは「袋から出して、載っているものと入れ替えて、袋へ戻す」が
//! 1 スレッド上で一息に終わる（却下案: coordinator と worker の往復で差し替える）。
//!
//! 予備が尽きて背景生成を待つときも、待つのは**その bank の worker** なので、
//! 反対の bank は render を続けられる。
//!
//! # 物理インスタンス数の上限
//! 種別 K の物理インスタンス数は「K に載っているスロット数 + 予備の数」。予備は
//! スロットが K から離れたときと、目標数に足りないときの背景生成でしか増えないので、
//! **`スロット数 + 予備の目標数` を超えない**。エビクションを持たないのはこのため。
//!
//! bank へ分けても総量は変えない。予備の目標数は
//! [`plan_bank_instances`] が**分離前の目標数を 2 bank へ割って**配る。
//!
//! # 予備は起動直後に前払いする
//! 予備の目標数はスロット数ぶん（上限 [`MAX_DEFAULT_SPARE_TARGET`]）で、起動直後の
//! 背景生成で埋める。使われはじめてから 1 本ずつ作ると、はじめて複数行が同時に
//! 別プラグインへ飛ぶ周で `prepare_slot_for_patch` が待たされる。総生成コストは変わらず、
//! 演奏中の待ちを演奏前のアイドルへ移すだけ。
//!
//! # 何を賭けているか
//! 背景生成は `cmrt_core::RendererHandoff`（`!Send` な CLAP インスタンスを unsafe に
//! スレッド間移送するラッパ）を演奏中ずっと踏む。詳細はそちらの doc コメント。

mod builder;

use std::time::{Duration, Instant};

use cmrt_core::RealtimeRenderer;

use crate::timing;

use super::bank::BANK_COUNT;
use cmrt_core::kind_for_patch;
pub(crate) use cmrt_core::{plugin_kinds, PatchBases, PluginKind};

use self::builder::{spawn_builder, BankBuilder};

/// 前払いする予備の上限。
///
/// 約 12MB/instance なので 8 個で約 96MB。Surge XT なら背景生成に約 4 秒かかるが、
/// それは起動完了後のアイドル時間で消化される（起動完了そのものは待たない）。
/// grid sequencer の実運用は 7 行なので、8 あれば初回の待ちが消える。
const MAX_DEFAULT_SPARE_TARGET: usize = 8;
const SPARE_TARGET_ENV: &str = "CMRT_SPARE_INSTANCES";

/// 予備が尽きて背景生成を待つときの上限。超えたら差し替えを失敗として返す。
const SPARE_WAIT_TIMEOUT: Duration = Duration::from_secs(20);

/// bank worker が自分の予備プールを作るための材料。
///
/// **物理インスタンスを含まない**ので、そのまま bank worker スレッドへ送れる
/// （`RendererHandoff` の unsafe な移送を新しく増やさない）。
pub(super) struct LiveInstancesSpec {
    bank: usize,
    kinds: Vec<PluginKind>,
    /// この bank が持つ論理スロットの数。
    slot_count: usize,
    spare_target: usize,
    builder: Option<BankBuilder>,
}

/// bank ごとの予備プールの材料を作る。
///
/// **2 bank の予備の目標数の合計は、分離前の目標数と同じ。**
/// `CMRT_SPARE_INSTANCES` の意味は「サーバー全体で持つ予備の数」のまま変えない。
/// 背景生成スレッドも 1 本だけ起こして両 bank で共有する。
pub(super) fn plan_bank_instances(
    kinds: Vec<PluginKind>,
    bank_slot_counts: [usize; BANK_COUNT],
) -> [LiveInstancesSpec; BANK_COUNT] {
    let total_slots = bank_slot_counts.iter().sum::<usize>();
    // 種別が 1 つしか無いなら、どの patch もプラグインをまたがない。背景スレッドも
    // 予備も要らない（Surge XT だけを入れている環境では今までと完全に同じ動きになる）。
    let total_target = if kinds.len() > 1 {
        spare_target(total_slots)
    } else {
        0
    };
    let targets = split_spare_target(total_target, bank_slot_counts);
    let mut builders = (total_target > 0).then(|| spawn_builder(kinds.clone()).into_iter());
    std::array::from_fn(|bank| LiveInstancesSpec {
        bank,
        kinds: kinds.clone(),
        slot_count: bank_slot_counts[bank],
        spare_target: targets[bank],
        builder: builders.as_mut().and_then(Iterator::next),
    })
}

/// サーバー全体の予備の目標数を bank へ割る。**合計は変えない。**
///
/// 端数は bank 0 へ付ける（`BankLayout` のスロットの割り方と同じ向き）。
/// スロットが 1 つも無い bank は差し替えようがないので、目標を相方へ寄せる。
fn split_spare_target(total: usize, bank_slot_counts: [usize; BANK_COUNT]) -> [usize; BANK_COUNT] {
    if bank_slot_counts[1] == 0 {
        return [total, 0];
    }
    [total.div_ceil(BANK_COUNT), total / BANK_COUNT]
}

pub(super) struct LiveInstances {
    /// 所有 bank。背景生成の返事が自分宛かを確かめるのに使う。
    bank: usize,
    kinds: Vec<PluginKind>,
    /// 音色無指定の行が鳴るプラグイン。`kinds` の添字。
    default_kind: usize,
    /// 論理スロット（bank-local index）→ いま載っている物理インスタンスの種別。
    slot_kind: Vec<usize>,
    /// 種別ごとの予備インスタンス。
    spares: Vec<Vec<RealtimeRenderer>>,
    /// 種別ごとの「発注済みで未着」の数。
    outstanding: Vec<usize>,
    spare_target: usize,
    builder: Option<BankBuilder>,
}

impl LiveInstances {
    /// すべてのスロットが既定プラグインに載っている状態から始める。
    ///
    /// **bank worker スレッドの上で呼ぶこと。** 予備の袋は `!Send` な物理インスタンスを
    /// 持つので、作った場所から動かせない。
    pub(super) fn new(spec: LiveInstancesSpec) -> Self {
        let LiveInstancesSpec {
            bank,
            kinds,
            slot_count,
            spare_target,
            builder,
        } = spec;
        let mut instances = Self {
            bank,
            slot_kind: vec![0; slot_count],
            spares: (0..kinds.len()).map(|_| Vec::new()).collect(),
            outstanding: vec![0; kinds.len()],
            default_kind: 0,
            kinds,
            spare_target,
            builder,
        };
        for kind in 0..instances.kinds.len() {
            instances.request_refill(kind);
        }
        instances
    }

    /// 背景で出来上がった予備を袋へ取り込む。ブロックしない。
    ///
    /// bank worker のコマンドループから毎周回呼ぶ。
    pub(super) fn collect_ready(&mut self) {
        let Some(builder) = self.builder.as_ref() else {
            return;
        };
        let ready = std::iter::from_fn(|| builder.try_recv()).collect::<Vec<_>>();
        for outcome in ready {
            self.accept(outcome);
        }
    }

    /// `patch` を載せるのに必要なプラグインをスロットへ用意する。
    ///
    /// 既に必要なプラグインが載っていれば何もしない。違えば、鳴っている音を止めてから
    /// 物理インスタンスを袋のものと入れ替える。
    ///
    /// `renderers` はこの bank の物理インスタンス（bank-local index 順）。台帳も袋も
    /// 同じ bank worker が持つので、取り出しと戻しがここで一息に終わる。
    ///
    /// 戻り値は**物理インスタンスの差し替えが起きたか**。
    pub(super) fn prepare_slot_for_patch(
        &mut self,
        renderers: &mut [RealtimeRenderer],
        slot: usize,
        patch: Option<&str>,
    ) -> Result<bool, String> {
        let wanted = kind_for_patch(&self.kinds, self.default_kind, patch)?;
        let current = self.slot_kind[slot];
        if current == wanted {
            return Ok(false);
        }
        let started = Instant::now();
        // 尽きていれば背景生成を待つ。待つのはこの bank の worker だけで、
        // 反対の bank は render を続けられる。
        let spare = self.take_spare(wanted)?;
        // 袋へ返す前に必ず音を止める。止めないと返した物理インスタンスが鳴りっぱなしになる。
        renderers[slot].reset();
        // ここで `set_patch(None)` はしない。state load は Dexed の program change guard を
        // armed にするため、次に取り出した直後の音色変更が捨てられうる。
        let evicted = std::mem::replace(&mut renderers[slot], spare);
        self.spares[current].push(evicted);
        self.slot_kind[slot] = wanted;
        self.request_refill(wanted);
        timing::log(&format!(
            "phase=instance_swap bank={} slot={slot} from={} to={} ms={} spares={} physical={}",
            self.bank,
            self.kinds[current].name,
            self.kinds[wanted].name,
            started.elapsed().as_millis(),
            self.spares[wanted].len(),
            self.physical_count(),
        ));
        Ok(true)
    }

    /// この bank の物理インスタンスの総数（スロットに載っているもの + 予備）。
    fn physical_count(&self) -> usize {
        self.slot_kind.len() + self.spares.iter().map(Vec::len).sum::<usize>()
    }

    /// 予備を 1 つ取り出す。無ければ背景生成を待つ。
    fn take_spare(&mut self, kind: usize) -> Result<RealtimeRenderer, String> {
        if let Some(spare) = self.spares[kind].pop() {
            return Ok(spare);
        }
        self.request_refill(kind);
        if self.outstanding[kind] == 0 {
            // 前払いの目標が 0 の bank（割り当てが相方へ寄った側）でも、いま要る 1 本は作る。
            self.order_build(kind);
        }
        let deadline = Instant::now() + SPARE_WAIT_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let received = match self.builder.as_ref() {
                Some(builder) if !remaining.is_zero() => builder.recv_timeout(remaining),
                Some(_) => Err(std::sync::mpsc::RecvTimeoutError::Timeout),
                None => {
                    return Err(format!(
                        "{} の予備インスタンスが無く、背景生成も動いていない",
                        self.kinds[kind].name
                    ))
                }
            };
            match received {
                Ok(outcome) => {
                    let failure = outcome.result.as_ref().err().cloned();
                    self.accept(outcome);
                    if let Some(spare) = self.spares[kind].pop() {
                        return Ok(spare);
                    }
                    if let Some(failure) = failure {
                        return Err(failure);
                    }
                }
                Err(_) => {
                    return Err(format!(
                        "{} のインスタンス生成が {} 秒で終わらなかった",
                        self.kinds[kind].name,
                        SPARE_WAIT_TIMEOUT.as_secs()
                    ))
                }
            }
        }
    }

    /// 予備が目標数に足りなければ背景生成を発注する。
    ///
    /// **足りないぶんを一度に積む。** 1 件ずつ積んで「届いたら次を積む」にすると、
    /// 受け取り（[`Self::collect_ready`]）が worker のループからしか呼ばれないため、
    /// コマンド待ちでブロックしているアイドル中に前払いが 1 個で止まる。
    /// 前払いはアイドル中に消化させたいものなので、発注は worker の都合から切り離す。
    fn request_refill(&mut self, kind: usize) {
        let target = self.spare_target_for(kind);
        while self.spares[kind].len() + self.outstanding[kind] < target {
            if !self.order_build(kind) {
                return;
            }
        }
    }

    /// 1 件だけ発注する。生成スレッドが居なければ `false`。
    fn order_build(&mut self, kind: usize) -> bool {
        let Some(builder) = self.builder.as_ref() else {
            return false;
        };
        if !builder.order(kind) {
            return false;
        }
        self.outstanding[kind] += 1;
        true
    }

    /// 種別ごとの予備の目標数。
    ///
    /// **既定プラグインだけは 0 でよい。** 起動時に全スロットが既定プラグインに載って
    /// いるので、種別 K の物理インスタンス数は「K に載っているスロット数 + 袋の中の数」で
    /// 一定。既定プラグインへ戻る要求が来るのは、どこかのスロットが既定から離れたあと
    /// だけで、そのとき離れた物理インスタンスは必ず袋に入っている。
    /// つまり既定プラグインの予備は自給自足で、背景生成は 1 度も要らない。
    ///
    /// 逆に既定プラグインの予備を先に発注すると、生成スレッドは 1 本しか無いので
    /// **本当に待たれている別プラグインの生成がその後ろに並ぶ**。実測では最初の
    /// 差し替えが Surge XT の 495ms ぶん丸ごと待たされていた。
    fn spare_target_for(&self, kind: usize) -> usize {
        if kind == self.default_kind {
            0
        } else {
            self.spare_target
        }
    }

    fn accept(&mut self, outcome: builder::BuildOutcome) {
        // 起きない（bank ごとに別の channel で受けている）。型で塞げない最後の一手として、
        // 他 bank の物理インスタンスを袋へ入れてしまわないよう確かめる。
        debug_assert_eq!(
            outcome.bank, self.bank,
            "bank {} の予備プールへ bank {} 宛の生成結果が届いた",
            self.bank, outcome.bank
        );
        if outcome.bank != self.bank {
            eprintln!(
                "cmrt-live: event=spare-bank-mismatch owner={} outcome={}",
                self.bank, outcome.bank
            );
            return;
        }
        self.outstanding[outcome.kind] = self.outstanding[outcome.kind].saturating_sub(1);
        match outcome.result {
            Ok(handoff) => {
                self.spares[outcome.kind].push(handoff.into_inner());
                timing::log(&format!(
                    "phase=spare_ready bank={} plugin={} ms={} spares={} physical={}",
                    self.bank,
                    self.kinds[outcome.kind].name,
                    outcome.elapsed.as_millis(),
                    self.spares[outcome.kind].len(),
                    self.physical_count(),
                ));
            }
            Err(error) => {
                eprintln!(
                    "cmrt-live: event=spare-build-failed bank={} plugin={} detail={error}",
                    self.bank, self.kinds[outcome.kind].name
                );
            }
        }
    }
}

/// プラグインごとに常時持っておく予備インスタンスの数（**サーバー全体で**）。
///
/// 1 周ごとの自動抽選（cycle random）は 1 周で複数行が同時に別プラグインへ飛ぶので、
/// 1 つだと 2 行目以降が背景生成を待つ。待つのは `prepare_slot_for_patch` を呼ぶ
/// bank worker スレッドで、Surge XT で 1 個約 490ms 止まる。
///
/// そこで**スロット数ぶん（上限 [`MAX_DEFAULT_SPARE_TARGET`]）を起動直後の背景で
/// 前払いする**。総生成コストは変わらないが、演奏が始まる前のアイドル時間へ移る。
/// スロット数で頭打ちにするのは、同時に別プラグインへ飛べるのがスロット数までで、
/// それ以上の予備は決して使われないため。
///
/// `CMRT_SPARE_INSTANCES` で上書きできる。`1` を渡せば前払いをやめた従来の挙動、
/// `0` を渡せば予備プールそのものを止められる。**bank へ分けても意味は変わらない**
/// （[`split_spare_target`] が合計をこの数に保つ）。
fn spare_target(slot_count: usize) -> usize {
    std::env::var(SPARE_TARGET_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or_else(|| slot_count.min(MAX_DEFAULT_SPARE_TARGET))
}

#[cfg(test)]
mod tests;
