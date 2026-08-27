//! 予備インスタンスを背景で作るスレッド。
//!
//! 演奏中の worker スレッドを止めずにインスタンスを増やすためだけの存在。
//! 1 本しか走らせないのは、並列に作っても速くならず取り合うだけだから
//! （`docs/adr/0012-measured-baselines.md`: 並列数 12 で 1 個あたり 350ms、単独なら 205ms）。
//! **bank ごとに 1 本ずつ起こしてはならない。** 予備プールは bank worker が持つように
//! なったが（Stage 4）、生成そのものは今までどおり 1 本のスレッドで直列に走らせる。
//! Vaporizer2 は複数インスタンスの同時生成で落ちるので、ここは直列性の要でもある
//! （`docs/adr/0013-serial-instantiation.md`）。
//!
//! # どの bank の発注か
//! 発注には bank を必ず添え、出来上がったものは**その bank 専用の channel** へ返す。
//! bank worker は自分の channel しか持たないので、他 bank の物理インスタンスを
//! 取り込みようがない（[`BankBuilder`] の型がそれを表している）。
//!
//! CLAP の entry（プラグイン本体の DLL）はこのスレッドが**要求されて初めて**ロードする。
//! そうしないと、使いもしないプラグインの `load_entry`（実測 112ms）が起動時間へ乗る。
//! entry はインスタンス側が clone を保持するので、このスレッドが終わって drop しても
//! 生きているインスタンスの足元は崩れない。

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use cmrt_core::{PluginEntry, RealtimeRenderer, RendererHandoff};

use crate::player::bank::BANK_COUNT;
use crate::timing;
use cmrt_core::PluginKind;

/// 1 件の発注。**bank を必ず添える**（返す先を間違えないため）。
struct BuildRequest {
    bank: usize,
    /// `kinds` の添字。
    kind: usize,
}

pub(super) struct BuildOutcome {
    /// 発注元の bank。受け取り側で自分宛かを確かめる。
    pub(super) bank: usize,
    /// `kinds` の添字。どの種別への発注だったか。
    pub(super) kind: usize,
    pub(super) result: Result<RendererHandoff, String>,
    pub(super) elapsed: Duration,
}

/// bank worker から見た背景生成の窓口。
///
/// 発注は全 bank 共通の 1 本のスレッドへ、受け取りは**自分の bank 専用**の channel から。
pub(super) struct BankBuilder {
    bank: usize,
    requests: Sender<BuildRequest>,
    outcomes: Receiver<BuildOutcome>,
}

impl BankBuilder {
    /// 1 件発注する。生成スレッドが居なければ `false`。
    pub(super) fn order(&self, kind: usize) -> bool {
        self.requests
            .send(BuildRequest {
                bank: self.bank,
                kind,
            })
            .is_ok()
    }

    /// 出来上がっていれば受け取る。待たない。
    pub(super) fn try_recv(&self) -> Option<BuildOutcome> {
        self.outcomes.try_recv().ok()
    }

    /// 出来上がるまで待って受け取る。
    pub(super) fn recv_timeout(&self, timeout: Duration) -> Result<BuildOutcome, RecvTimeoutError> {
        self.outcomes.recv_timeout(timeout)
    }
}

/// 背景生成スレッドを 1 本だけ起こし、bank ごとの窓口を返す。
///
/// スレッドは全 [`BankBuilder`] が落ちると止まる（発注 channel の送信側が全滅するため）。
/// join ハンドルを持たないのは、bank worker が終わるのがプロセス終了時だけだから。
pub(super) fn spawn_builder(kinds: Vec<PluginKind>) -> [BankBuilder; BANK_COUNT] {
    let (request_tx, request_rx) = std::sync::mpsc::channel::<BuildRequest>();
    let mut outcome_txs = Vec::with_capacity(BANK_COUNT);
    let mut outcome_rxs = Vec::with_capacity(BANK_COUNT);
    for _ in 0..BANK_COUNT {
        let (tx, rx) = std::sync::mpsc::channel::<BuildOutcome>();
        outcome_txs.push(tx);
        outcome_rxs.push(rx);
    }
    let spawned = std::thread::Builder::new()
        .name("realtime-play-server-spare-builder".to_string())
        .spawn(move || run_builder(kinds, &request_rx, &outcome_txs));
    if let Err(error) = spawned {
        // 予備が作れないだけで、既存のスロットは動き続ける。差し替え要求が来たときに
        // 「予備が無い」というエラーとして表面化する。
        eprintln!("cmrt-live: event=spare-builder-spawn-failed detail={error}");
    }
    let mut outcome_rxs = outcome_rxs.into_iter();
    std::array::from_fn(|bank| BankBuilder {
        bank,
        requests: request_tx.clone(),
        outcomes: outcome_rxs.next().expect("bank の数だけ作ってある"),
    })
}

fn run_builder(
    kinds: Vec<PluginKind>,
    requests: &Receiver<BuildRequest>,
    outcomes: &[Sender<BuildOutcome>],
) {
    let mut entries: Vec<Option<PluginEntry>> = vec![None; kinds.len()];
    while let Ok(request) = requests.recv() {
        let BuildRequest { bank, kind } = request;
        let started = Instant::now();
        let result = build_one(&kinds[kind], &mut entries[kind]);
        // worker が拾うのは次にコマンドが来たときなので、生成そのものはここで記録する。
        // そうしないと、アイドル中に走った背景生成がログに現れない。
        timing::log(&format!(
            "phase=spare_built bank={bank} plugin={} ms={} result={}",
            kinds[kind].name,
            started.elapsed().as_millis(),
            if result.is_ok() { "ok" } else { "failed" },
        ));
        let outcome = BuildOutcome {
            bank,
            kind,
            result,
            elapsed: started.elapsed(),
        };
        // 発注元の bank だけへ返す。その bank が畳まれていても、他の bank の発注は
        // 続くのでスレッドは止めない。
        let _ = outcomes[bank].send(outcome);
    }
}

fn build_one(
    kind: &PluginKind,
    entry: &mut Option<PluginEntry>,
) -> Result<RendererHandoff, String> {
    if entry.is_none() {
        let loaded = cmrt_core::load_entry(&kind.plugin_path)
            .map_err(|error| format!("{error:#} (plugin_path={})", kind.plugin_path))?;
        *entry = Some(loaded);
    }
    let entry = entry.as_ref().expect("entry was just loaded");
    RealtimeRenderer::new(&kind.core_cfg, entry)
        .map(RendererHandoff::new)
        .map_err(|error| format!("{error:#} (plugin_path={})", kind.plugin_path))
}
