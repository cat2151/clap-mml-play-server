//! インスタンス生成を、耐えないプラグインのためだけに直列化する。
//!
//! Vaporizer2 は instance をスレッド並列に作ると `STATUS_ACCESS_VIOLATION` でプロセスごと落ちる。
//! プロセス共通の `RwLock` 1 本で、直列化が要らないプラグインは **read**（並列のまま）、要る
//! プラグインは **write**（自分どうしも他プラグインとも重ならない）を取る。理由・却下案・A/B の
//! 手順は `docs/adr/0013-serial-instantiation.md`。
//!
//! 守るのは `create_plugin()` + `init()` の区間だけ。Vaporizer2 がコンストラクタから投げる非同期の
//! プリセット走査は待たない（待たない形で通ることを確かめてあり、待つ形は確かめていない）。

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::vvp::VAPORIZER2_PLUGIN_ID;

/// 直列化をやめる環境変数（`off` / `0` / `false` で無効）。**A/B を測るためだけにある**
/// （`parallel_instance_creation` example で segfault が再現することを示す）。
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
