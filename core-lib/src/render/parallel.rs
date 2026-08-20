//! 複数の `RealtimeRenderer` をまとめて並列に生成する。
//!
//! 再生サーバーの起動時間はほぼ全部が CLAP インスタンス生成で、その中身（Surge XT の `init()`）は
//! ほぼ完全に CPU 律速の単一スレッド処理（実測 cpu/wall = 0.93）。I/O 待ちではないので、
//! コアを使い切れば実時間が縮む。
//!
//! index ごとに別の `(cfg, entry)` を渡せるようにしてあるのは、1 プロセスの中で
//! Surge XT と Dexed のインスタンスを混在させるため（`docs/adr/0008-spare-instance-pool.md`）。

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, MutexGuard,
};

use anyhow::Result;
use clack_host::prelude::*;

use super::{RealtimeRenderer, RendererInitTiming};
use crate::CoreConfig;

/// インスタンス 1 つを作るための材料。index ごとに別のプラグインを指してよい。
#[derive(Clone, Copy)]
pub struct RendererSpec<'a> {
    pub cfg: &'a CoreConfig,
    pub entry: &'a PluginEntry,
}

/// インスタンスが 1 つ出来上がるたびに呼び出し側へ渡す情報。
///
/// 生成順は不定なので、進捗表示には `index` ではなく `completed` を使うこと。
#[derive(Clone, Copy, Debug)]
pub struct RendererCreated {
    /// 何番のインスタンスか。返る `Vec` はこの順に並んでいる。
    pub index: usize,
    /// これで何個目が完成したか（1 始まり）。
    pub completed: usize,
    /// 総数。
    pub total: usize,
    pub timing: RendererInitTiming,
}

/// 生成スレッドから別スレッドへ `RealtimeRenderer` を移すためのラッパ。
///
/// # 何を賭けているか（安全性の証明ではない）
/// `RealtimeRenderer` は clack が `!Send` に封じている `PluginInstance`（main-thread ハンドル）を
/// 持つ。CLAP の規約では `init()` したスレッドと、以後 `clap_plugin_state.load`（= `set_patch`）を
/// 呼ぶスレッドは同じ「main thread」でなければならず、ここではそれを破っている。
///
/// つまりこれは **「CLAP ラッパはスレッド同一性を実際には要求しない」という賭け**であり、
/// 賭けが外れると patch 切替やインスタンス破棄で壊れる。プラグインのバージョンが上がって
/// 壊れる可能性も残っている。実測での確認内容は `startup-speedup-handoff.md` を参照。
///
/// # 起動時だけでなく演奏中も踏むようになった
/// 予備インスタンスプール（`docs/adr/0008-spare-instance-pool.md`）は、演奏中に背景スレッドで
/// インスタンスを作り続けて worker スレッドへ渡す。この賭けの露出は起動時 1 回から
/// 演奏中ずっとへ広がっている。壊れ方の観測点はこの型を使う側のログ。
pub struct RendererHandoff(RealtimeRenderer);

// SAFETY: 上記のとおり型システム上の保証ではなく、CLAP ラッパの実装への賭け。
unsafe impl Send for RendererHandoff {}

impl RendererHandoff {
    pub fn new(renderer: RealtimeRenderer) -> Self {
        Self(renderer)
    }

    pub fn into_inner(self) -> RealtimeRenderer {
        self.0
    }
}

/// `specs` と同数の `RealtimeRenderer` を最大 `threads` 本のスレッドで生成し、index 順に並べて返す。
///
/// `on_ready` は生成スレッドから、完成のたびに直列化して呼ばれる。
/// 1 つでも生成に失敗したらその時点で残りを打ち切り、`Err` を返す。
pub fn create_renderers_parallel(
    specs: &[RendererSpec<'_>],
    threads: usize,
    on_ready: &(dyn Fn(RendererCreated) + Sync),
) -> Result<Vec<RealtimeRenderer>> {
    let count = specs.len();
    if count == 0 {
        return Ok(Vec::new());
    }
    let threads = threads.clamp(1, count);
    let next_index = AtomicUsize::new(0);
    let created: Mutex<Vec<(usize, RendererHandoff)>> = Mutex::new(Vec::with_capacity(count));
    let failure: Mutex<Option<(usize, anyhow::Error)>> = Mutex::new(None);

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                if lock(&failure).is_some() {
                    break;
                }
                let index = next_index.fetch_add(1, Ordering::Relaxed);
                if index >= count {
                    break;
                }
                let spec = specs[index];
                match RealtimeRenderer::new_with_timing(spec.cfg, spec.entry) {
                    Ok((renderer, timing)) => {
                        // ログ行が入り乱れないよう、通知はロックの中で直列化する。
                        let mut created = lock(&created);
                        created.push((index, RendererHandoff(renderer)));
                        on_ready(RendererCreated {
                            index,
                            completed: created.len(),
                            total: count,
                            timing,
                        });
                    }
                    Err(error) => {
                        *lock(&failure) = Some((index, error));
                        break;
                    }
                }
            });
        }
    });

    if let Some((index, error)) = lock(&failure).take() {
        return Err(error.context(format!("CLAP インスタンス {index} の生成に失敗")));
    }
    let mut created = std::mem::take(&mut *lock(&created));
    created.sort_by_key(|(index, _)| *index);
    Ok(created
        .into_iter()
        .map(|(_, renderer)| renderer.0)
        .collect())
}

/// 生成スレッドが panic しても残りのスレッドが動き続けられるよう、毒は無視する。
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}
