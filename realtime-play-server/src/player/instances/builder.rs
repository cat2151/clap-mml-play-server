//! 予備インスタンスを背景で作るスレッド。
//!
//! 演奏中の worker スレッドを止めずにインスタンスを増やすためだけの存在。
//! 1 本しか走らせないのは、並列に作っても速くならず取り合うだけだから
//! （`docs/adr/0012-measured-baselines.md`: 並列数 12 で 1 個あたり 350ms、単独なら 205ms）。
//!
//! CLAP の entry（プラグイン本体の DLL）はこのスレッドが**要求されて初めて**ロードする。
//! そうしないと、使いもしないプラグインの `load_entry`（実測 112ms）が起動時間へ乗る。
//! entry はインスタンス側が clone を保持するので、このスレッドが終わって drop しても
//! 生きているインスタンスの足元は崩れない。

use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use cmrt_core::{PluginEntry, RealtimeRenderer, RendererHandoff};

use crate::timing;
use cmrt_core::PluginKind;

pub(super) struct BuildOutcome {
    /// `kinds` の添字。どの種別への発注だったか。
    pub(super) kind: usize,
    pub(super) result: Result<RendererHandoff, String>,
    pub(super) elapsed: Duration,
}

pub(super) struct BuilderHandle {
    /// 作ってほしい種別の添字を送る。
    pub(super) requests: Sender<usize>,
    pub(super) outcomes: Receiver<BuildOutcome>,
}

/// 背景生成スレッドを起こす。
///
/// スレッドは `requests` の送信側が落ちると止まる。[`BuilderHandle`] を drop すれば
/// 畳まれるので、join ハンドルは持たない（worker が終わるのはプロセス終了時だけ）。
pub(super) fn spawn_builder(kinds: Vec<PluginKind>) -> BuilderHandle {
    let (request_tx, request_rx) = std::sync::mpsc::channel::<usize>();
    let (outcome_tx, outcome_rx) = std::sync::mpsc::channel::<BuildOutcome>();
    let spawned = std::thread::Builder::new()
        .name("realtime-play-server-spare-builder".to_string())
        .spawn(move || run_builder(kinds, &request_rx, &outcome_tx));
    if let Err(error) = spawned {
        // 予備が作れないだけで、既存のスロットは動き続ける。差し替え要求が来たときに
        // 「予備が無い」というエラーとして表面化する。
        eprintln!("cmrt-live: event=spare-builder-spawn-failed detail={error}");
    }
    BuilderHandle {
        requests: request_tx,
        outcomes: outcome_rx,
    }
}

fn run_builder(
    kinds: Vec<PluginKind>,
    requests: &Receiver<usize>,
    outcomes: &Sender<BuildOutcome>,
) {
    let mut entries: Vec<Option<PluginEntry>> = vec![None; kinds.len()];
    while let Ok(kind) = requests.recv() {
        let started = Instant::now();
        let result = build_one(&kinds[kind], &mut entries[kind]);
        // worker が拾うのは次にコマンドが来たときなので、生成そのものはここで記録する。
        // そうしないと、アイドル中に走った背景生成がログに現れない。
        timing::log(&format!(
            "phase=spare_built plugin={} ms={} result={}",
            kinds[kind].name,
            started.elapsed().as_millis(),
            if result.is_ok() { "ok" } else { "failed" },
        ));
        let outcome = BuildOutcome {
            kind,
            result,
            elapsed: started.elapsed(),
        };
        if outcomes.send(outcome).is_err() {
            break;
        }
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
