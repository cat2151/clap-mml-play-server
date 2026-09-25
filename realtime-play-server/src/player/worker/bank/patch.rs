//! 音色ロードを bank worker へ投げ、その返事を引き取るところ。
//!
//! # 送るだけで戻れること
//! 先読み（[`BankWorkers::start_patch`]）は**送るだけで戻る**。coordinator はその間も
//! 演奏 bank の render を回し、毎周回 [`BankWorkers::try_finish_patch`] で返事を拾う。
//! 従来の同期ロード（現在 bank の手動変更・起動時 prepare・voicing probe）は
//! 同じ仕組みを「送ってすぐ待つ」形で使うので、意味は変わらない。
//!
//! # 予備インスタンスはここを通らない
//! プラグイン種別が変わる差し替えも、予備の袋から取り出すのも、押し出された 1 本を
//! 袋へ戻すのも、すべて**対象 bank worker の中で完結する**。coordinator は
//! 「どの instance へどの音色を」しか知らない。却下案: ここが袋の出し入れを担う
//! （返事を引き取るまで台帳が差し替え前のままという非対称が出る）。

use cmrt_core::VoicingReport;

use super::protocol::{BankCommand, BankReply, PatchJob};
use super::{BankSlot, BankWorkers};

/// 送信済みで、まだ返事を受け取っていない音色ロード。
///
/// これを持っているあいだ、その bank へ他の仕事を送ってはならない
/// （返事が 1 本の channel に相乗りしているため）。
pub(in super::super) struct PendingPatch {
    slot: BankSlot,
}

impl BankWorkers {
    /// 先読みロードを bank worker へ投げて、**返事を待たずに**戻る。
    ///
    /// 呼び出し側は返ってきた [`PendingPatch`] を持ち続け、
    /// [`Self::try_finish_patch`] で返事を拾うこと。
    pub(in super::super) fn start_patch(
        &self,
        instance_index: usize,
        patch: Option<&str>,
        effect_chain: &str,
    ) -> Result<PendingPatch, String> {
        self.send_patch_job(instance_index, patch, effect_chain, true, true)
    }

    /// 返事が来ていれば引き取る。来ていなければ `pending` を触らずに `None`。
    pub(in super::super) fn try_finish_patch(
        &self,
        pending: &mut Option<PendingPatch>,
    ) -> Option<Result<(), String>> {
        let bank = pending.as_ref()?.slot.bank;
        let reply = self.workers[bank].try_receive()?;
        *pending = None;
        Some(absorb_patch_reply(bank, reply))
    }

    /// 返事が来るまで待って引き取る。演奏が止まっているとき（何も render していない
    /// とき）と、同期ロードの経路で使う。
    pub(in super::super) fn finish_patch(
        &self,
        pending: &mut Option<PendingPatch>,
    ) -> Option<Result<(), String>> {
        let bank = pending.as_ref()?.slot.bank;
        let reply = self.workers[bank].receive();
        *pending = None;
        Some(absorb_patch_reply(bank, reply))
    }

    /// 音色と effect chain を載せて完了まで待つ。`settle` が真なら反映のため 4 ブロック空回しする。
    ///
    /// 現在 bank の手動変更・MML overlay・起動時 prepare が使う既存の経路。
    pub(in super::super) fn prepare_patch(
        &self,
        instance_index: usize,
        patch: Option<&str>,
        effect_chain: &str,
        settle: bool,
    ) -> Result<(), String> {
        let mut pending =
            Some(self.send_patch_job(instance_index, patch, effect_chain, true, settle)?);
        self.finish_patch(&mut pending)
            .expect("送った仕事には返事が 1 つある")
    }

    /// scheduled 再生の頭で、instance 0 へ音色を載せる（settle しない従来どおりの経路）。
    pub(in super::super) fn prepare_scheduled_patch(
        &self,
        patch: Option<&str>,
    ) -> Result<(), String> {
        let mut pending = Some(self.send_patch_job(0, patch, "", false, false)?);
        self.finish_patch(&mut pending)
            .expect("送った仕事には返事が 1 つある")
    }

    /// 音色を載せたうえで、そのプラグインの発音能力を測る。
    pub(in super::super) fn probe_patch(
        &self,
        instance_index: usize,
        patch: Option<&str>,
    ) -> Result<VoicingReport, String> {
        let (slot, job) = self.build_patch_job(instance_index, patch, "", true, false);
        let reply = self.workers[slot.bank]
            .request(BankCommand::ProbePatch(job))
            .map_err(|error| format!("{error:#}"))?;
        let BankReply::Probed(outcome) = reply else {
            return Err(format!(
                "bank {} worker returned an unexpected reply",
                slot.bank
            ));
        };
        outcome.result
    }

    /// 仕事を組み立てて送る。ここでは返事を待たない。
    fn send_patch_job(
        &self,
        instance_index: usize,
        patch: Option<&str>,
        effect_chain: &str,
        reset_before: bool,
        settle: bool,
    ) -> Result<PendingPatch, String> {
        let (slot, job) =
            self.build_patch_job(instance_index, patch, effect_chain, reset_before, settle);
        self.workers[slot.bank]
            .send(BankCommand::PreparePatch(job))
            .map_err(|error| format!("{error:#}"))?;
        Ok(PendingPatch { slot })
    }

    /// global instance index を所有 bank と bank-local index へ直し、仕事を組み立てる。
    fn build_patch_job(
        &self,
        instance_index: usize,
        patch: Option<&str>,
        effect_chain: &str,
        reset_before: bool,
        settle: bool,
    ) -> (BankSlot, PatchJob) {
        let slot = self.layout.slot_of_index(instance_index);
        (
            slot,
            PatchJob {
                local_index: slot.local_index,
                patch: patch.map(str::to_string),
                effect_chain: effect_chain.to_string(),
                reset_before,
                settle,
            },
        )
    }
}

/// ロードの成否を取り出す。
fn absorb_patch_reply(bank: usize, reply: anyhow::Result<BankReply>) -> Result<(), String> {
    let reply = reply.map_err(|error| format!("{error:#}"))?;
    let BankReply::Patched(outcome) = reply else {
        return Err(format!("bank {bank} worker returned an unexpected reply"));
    };
    outcome.result
}
