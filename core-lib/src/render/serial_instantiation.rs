//! インスタンス生成を、耐えないプラグインのためだけに直列化する。
//!
//! # なぜ要るか（実測）
//! `create_renderers_parallel`（起動時 8 スレッド）と予備インスタンスプールの背景スレッド、
//! それにオフライン render server の worker は、**1 つの entry を共有したまま複数スレッドで
//! instance を作る**。CLAP の規約では instance 生成は main thread 限定なので、これは
//! もともと賭け（`docs/adr/0009-unsafe-thread-handoff.md`）で、Surge XT と Dexed では当たっていた。
//!
//! **Vaporizer2 3.5.0 で賭けが外れた。** 2 スレッドでも `STATUS_ACCESS_VIOLATION` で
//! プロセスごと落ちる（3/3 で再現。entry を共有しなくても落ちるので `PluginEntry::load` の
//! 競合ではなく instance 生成そのもの）。直列なら 8 個でも通る。確認口:
//!
//! ```text
//! cargo run --release --example parallel_instance_creation -- "<CLAP のパス>" 8
//! ```
//!
//! # どう直列化するか
//! 「Vaporizer2 の生成中は、他のどのプラグインの生成も走っていない」を保証する。
//! そのため `RwLock` を使い、
//!
//! - 直列化が要らないプラグイン → **read**（今までどおり並列に作れる）
//! - 直列化が要るプラグイン → **write**（自分どうしも、他プラグインとも重ならない）
//!
//! とする。read 同士は競合しないので、**Surge XT だけ / Dexed だけの構成の速度は変わらない**。
//! 「他プラグインとも重ならない」まで広げているのは、混ざったときに落ちないことを
//! 別途実測していないため。安い側（read 1 回）へ倒している。
//!
//! # 何を守り、何を守らないか
//! 守るのは `create_plugin()` + `init()` の区間だけ。Vaporizer2 のコンストラクタは
//! プリセット走査（`reloadPresetArray`）を**非同期スレッドへ投げる**ので、生成から戻った
//! あともしばらく裏で走っている。それを待たないのは、上の実測で「直列生成」と呼んでいるもの
//! （= 次の生成が前の走査と重なる形）がまさにそれで、その形が通ることを確かめてあるから。

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::vvp::VAPORIZER2_PLUGIN_ID;

/// 直列化をやめる環境変数（`off` / `0` / `false` で無効）。
///
/// **A/B を測るためだけにある。** これを渡して
/// `parallel_instance_creation` を Vaporizer2 で走らせると、直列化を入れる前と同じ
/// segfault が再現する。つまり「落ちなくなったのはこの実装のおかげ」を機械で示せる。
const SERIAL_ENV: &str = "CMRT_SERIAL_INSTANTIATION";

/// 生成中であることを表すプロセス共通のロック。
static INSTANTIATION: RwLock<()> = RwLock::new(());

/// このプラグインは instance をスレッド並列に作れないか。
///
/// 材料は CLAP descriptor の ID（＝実際にロードされた本物の ID）で、config の推測ではない。
/// 表に載っていないプラグインは今までどおり並列に作る。
pub fn plugin_requires_serial_instantiation(plugin_id: &str) -> bool {
    plugin_id == VAPORIZER2_PLUGIN_ID
}

/// 生成区間を抜けるまで保持するトークン。
///
/// `Drop` で解放されるだけなので、呼び出し側は生成が終わるまで束縛しておくこと。
pub(super) enum InstantiationPermit {
    /// 並列に作ってよいプラグイン。他の read とは同時に通る。
    Shared(#[allow(dead_code)] RwLockReadGuard<'static, ()>),
    /// 直列化が要るプラグイン。他のどの生成とも重ならない。
    Exclusive(#[allow(dead_code)] RwLockWriteGuard<'static, ()>),
    /// 環境変数で直列化を切った状態。ロックを一切取らない。
    Disabled,
}

impl InstantiationPermit {
    pub(super) fn acquire(plugin_id: &str) -> Self {
        if !serialization_enabled() {
            return Self::Disabled;
        }
        if plugin_requires_serial_instantiation(plugin_id) {
            // 生成スレッドが panic して毒されていても、残りのスレッドは作り続けてよい。
            // 守っているのは「同時に走らせない」ことだけで、共有データは無い。
            Self::Exclusive(INSTANTIATION.write().unwrap_or_else(|e| e.into_inner()))
        } else {
            Self::Shared(INSTANTIATION.read().unwrap_or_else(|e| e.into_inner()))
        }
    }
}

fn serialization_enabled() -> bool {
    serialization_enabled_for(std::env::var(SERIAL_ENV).ok().as_deref())
}

/// 環境変数の値だけを見る判定。**未設定は有効**（何も設定していない実運用が守られる側）。
fn serialization_enabled_for(value: Option<&str>) -> bool {
    !matches!(
        value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "off" | "0" | "false"
    )
}

#[cfg(test)]
mod tests;
