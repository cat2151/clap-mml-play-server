//! 先読みロードの進行管理（coordinator 側）。
//!
//! 先読み（`PrepareStandbyPatch`）を受けた coordinator は、
//!
//! 1. その bank の未消化イベントと auto gain を落とし、
//! 2. ロードを bank worker へ**送るだけ**で戻る。
//!
//! 送った bank は返事を受け取るまで「塞がっている」ので render 対象から外れ
//! （`BankWorkers::render_enabled_for`）、演奏 bank だけが毎ブロック回り続ける。
//! 返事はレンダーループの毎周回 [`poll`] で拾う。**どこかで待つとしたら、何も
//! render していないときだけ**（[`settle`]）。
//!
//! # render 対象へ戻るのはいつか
//! ロードの返事を引き取った時点。「その bank 宛の MIDI が来たら戻す」という形にすると、
//! ロード中に届いた MIDI を取りこぼしたときにその bank が二度と鳴らなくなる。
//! 戻したあとの待機 bank が render するのは、先読み済み instance の無音と、
//! まだ先読みしていない instance の減衰だけで、分離前と同じ量である。

use std::{sync::mpsc::SyncSender, time::Instant};

use cmrt_realtime_ipc::InstanceId;

use super::bank::{BankWorkers, PendingPatch};
use super::command::ensure_live_mode;
use super::{AudioOutputControl, AutoGainControl, LimiterMeterState, MasterLimiter, PlaybackMode};

/// 先読みの要求そのもの。
///
/// `completion` は容量 1 の同期チャネル（`player::standby_completion_channel`）。
/// 受け取り手（IPC 受信スレッド）は非 blocking に poll するだけなので、送る側で
/// 待たないことが必須。
pub(super) struct StandbyRequest {
    pub(super) generation: u64,
    pub(super) instance_id: InstanceId,
    pub(super) patch: Option<String>,
    pub(super) effect_chain: String,
    pub(super) completion: SyncSender<Result<(), String>>,
}

/// 送信済みで、まだ返事を受け取っていない先読みロード。
pub(super) struct StandbyLoad {
    /// 返事を引き取ると `None` になる（`BankWorkers::try_finish_patch` が消費する）。
    pending: Option<PendingPatch>,
    bank: usize,
    instance_index: usize,
    started: Instant,
    /// 開始時点の「対象 bank 以外が render したブロック数」と underrun frames。
    /// 完了時の差分が「ロード中も演奏が進んだか」の機械判定になる。
    blocks_elsewhere_at_start: u64,
    underrun_at_start: u64,
    /// 開始時点の「この bank が塞がっていて render を出せなかった instance 数」。
    render_skips_at_start: u64,
    completion: SyncSender<Result<(), String>>,
}

/// 先読みの進行に必要な coordinator の持ち物。
pub(super) struct StandbyContext<'a> {
    pub(super) banks: &'a BankWorkers,
    pub(super) limiter: &'a mut MasterLimiter,
    pub(super) limiter_meter: &'a LimiterMeterState,
    pub(super) auto_gain: &'a AutoGainControl,
    pub(super) audio_output: &'a AudioOutputControl,
    pub(super) playback_mode: &'a mut Option<PlaybackMode>,
}

/// いま鳴らしている位置（サンプル）。live で無ければ 0。
fn live_clock(playback_mode: &Option<PlaybackMode>) -> u64 {
    match playback_mode {
        Some(PlaybackMode::Live { clock_samples, .. }) => *clock_samples,
        _ => 0,
    }
}

/// 先読みを始める。**ロードの完了は待たない。**
pub(super) fn begin(
    ctx: &mut StandbyContext<'_>,
    standby: &mut Option<StandbyLoad>,
    request: StandbyRequest,
) {
    // 先読みは 1 件ずつ完了を待つ契約（計画の「前提と守る契約」3）なので通常は起きないが、
    // 重なったら前のものを先に畳む。返事の対応が 1 本の channel で決まっているため。
    settle(ctx, standby);
    let index = usize::from(request.instance_id);
    ensure_live_mode(
        ctx.playback_mode,
        request.generation,
        ctx.banks.instance_count(),
    );
    let bank = ctx.banks.bank_of(index);
    // 「発音 deadline を越えた待機 bank だ」というクライアントの宣言を、サーバー側で
    // 表現する。溜まったままのイベントを落とさないと、render 対象へ戻った瞬間に
    // 発音時刻を過ぎたイベントがまとめて鳴る。
    clear_bank_playback_state(ctx, bank);
    let blocks_elsewhere_at_start = ctx.banks.blocks_rendered_elsewhere(bank);
    let underrun_at_start = ctx.audio_output.underrun_frames();
    let render_skips_at_start = ctx.banks.render_skips(bank);
    match ctx
        .banks
        .start_patch(index, request.patch.as_deref(), &request.effect_chain)
    {
        Ok(pending) => {
            // `clock` は「このスロットを書き換えた瞬間の再生位置」。**note on の
            // 予約位置（クライアント側の `at_frames`）と突き合わせるためにある。**
            // 予約位置より後ろの clock で書き換えていたら、鳴る前に上書きしたということ。
            let clock = live_clock(ctx.playback_mode);
            eprintln!(
                "cmrt-standby-load: bank={bank} event=start instance={index} clock={clock} \
                 blocks_elsewhere={blocks_elsewhere_at_start} underrun_frames={underrun_at_start}"
            );
            *standby = Some(StandbyLoad {
                pending: Some(pending),
                bank,
                instance_index: index,
                started: Instant::now(),
                blocks_elsewhere_at_start,
                underrun_at_start,
                render_skips_at_start,
                completion: request.completion,
            });
        }
        Err(error) => {
            eprintln!(
                "cmrt-standby-load: bank={bank} event=start-failed instance={index} detail={error}"
            );
            let _ = request.completion.send(Err(error));
        }
    }
}

/// 返事が来ていれば引き取る。**来ていなければ何もせずに戻る。**
pub(super) fn poll(ctx: &mut StandbyContext<'_>, standby: &mut Option<StandbyLoad>) {
    let Some(load) = standby.as_mut() else {
        return;
    };
    let Some(result) = ctx.banks.try_finish_patch(&mut load.pending) else {
        return;
    };
    let load = standby.take().expect("直前に Some を確かめている");
    complete(ctx, load, result);
}

/// 返事が来るまで待って引き取る。
///
/// **何も render していないときと、その bank へ同期の仕事を出す直前にだけ呼ぶこと。**
/// レンダーループが `wait_for_command()` で眠るとロードの返事を誰も拾わなくなり、
/// クライアントが timeout まで返らない。
pub(super) fn settle(ctx: &mut StandbyContext<'_>, standby: &mut Option<StandbyLoad>) {
    let Some(load) = standby.as_mut() else {
        return;
    };
    let Some(result) = ctx.banks.finish_patch(&mut load.pending) else {
        return;
    };
    let load = standby.take().expect("直前に Some を確かめている");
    complete(ctx, load, result);
}

/// 先読みの完了を台帳と live 状態へ反映し、受付票へ結果を落とす。
///
/// `completion` は容量 1 なので、この `send` は受け取り手の有無に関わらず
/// 即座に戻る。**ここが block するとレンダーループごと止まる。**
fn complete(ctx: &mut StandbyContext<'_>, load: StandbyLoad, result: Result<(), String>) {
    let blocks_elsewhere = ctx
        .banks
        .blocks_rendered_elsewhere(load.bank)
        .saturating_sub(load.blocks_elsewhere_at_start);
    let underrun_frames = ctx
        .audio_output
        .underrun_frames()
        .saturating_sub(load.underrun_at_start);
    // 0 でなければ、鳴っている bank へ先読みを送っている（クライアント側の契約違反）。
    let skipped = ctx
        .banks
        .render_skips(load.bank)
        .saturating_sub(load.render_skips_at_start);
    // 「ロード中も演奏が進んだか」の機械判定に使う行。演奏 bank が何ブロック進んだか、
    // その間に underrun が増えたかを 1 行で出す。
    eprintln!(
        "cmrt-standby-load: bank={} event=finish instance={} elapsed_ms={} \
         blocks_elsewhere={blocks_elsewhere} underrun_frames={underrun_frames} skipped={skipped} result={}",
        load.bank,
        load.instance_index,
        load.started.elapsed().as_millis(),
        if result.is_ok() { "ok" } else { "error" },
    );
    if result.is_err() {
        mark_failed_instance(ctx, load.instance_index);
    }
    let _ = load.completion.send(result);
}

/// ロードに失敗した instance は鳴らせない。既存の同期ロードと同じ後始末をする。
fn mark_failed_instance(ctx: &mut StandbyContext<'_>, index: usize) {
    let Some(PlaybackMode::Live { instances, .. }) = ctx.playback_mode.as_mut() else {
        return;
    };
    instances[index].active = false;
    if instances.iter().any(|instance| instance.active) {
        return;
    }
    ctx.audio_output.finish();
    ctx.limiter.reset();
    ctx.limiter_meter.reset();
    *ctx.playback_mode = None;
}

/// 待機 bank の未消化イベントと auto gain を落とす。
fn clear_bank_playback_state(ctx: &mut StandbyContext<'_>, bank: usize) {
    let banks = ctx.banks;
    let auto_gain = ctx.auto_gain;
    let Some(PlaybackMode::Live { instances, .. }) = ctx.playback_mode.as_mut() else {
        return;
    };
    for (index, instance) in instances.iter_mut().enumerate() {
        if banks.bank_of(index) != bank {
            continue;
        }
        instance.queue.clear();
        instance.auto_gain.reset();
        auto_gain.set_gain_db(index, 0.0);
    }
}
