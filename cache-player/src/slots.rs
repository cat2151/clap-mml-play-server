//! スロット（同時に載せておけるキャッシュ WAV）と、それを指示する CLAP state の綴り。
//!
//! # なぜスロットが要るのか
//! DAW の演奏は小節ごとにキャッシュ WAV を差し替える。1 instance = 1 WAV のままだと、
//! 差し替えは**小節境界に到達してから**しか出せず、その state load（実測 100〜130ms）が
//! まるごと小節の頭の無音になる。「小節 N を鳴らしている最中に小節 N+1 を載せておく」
//! ための最小構成が **2 スロット**（現在の小節 + 次の小節）。
//!
//! **ただし最小構成では余裕が 1 小節しか無い。** 実際に採ってあるのは 4 スロットで、
//! その理由は [`SLOT_COUNT`] の doc に書いてある。
//!
//! 増やすのは track 方向ではなく**時間方向だけ**。gain は
//! `set_live_instance_gain_db(instance_id, ..)` と instance 単位なので、
//! 1 演奏 track = 1 live instance は維持する。
//!
//! # note number でスロットを選ぶ
//! [`slot_for_note`] を参照。
//!
//! # state の綴り
//! **綴りの定義（単一ソース）は `core-lib/src/cache_wav.rs` の module doc。**
//! ここはその parser で、[`parse_state`] / [`slot_patch_state`] /
//! [`split_slot_prefix`] が実装にあたる。綴りを変えるときは向こうの doc も直すこと。

use std::sync::Arc;

use crate::buffer::CacheBuffer;
use crate::graveyard::BufferGraveyard;

/// 同時に載せておけるキャッシュ WAV の本数。
///
/// **鳴っている音を保持する仕組みではない**ので、余韻の長さとは無関係
/// （voice は自分が握った `Arc` を鳴らし続ける）。効くのは
/// **「先読みが、まだ鳴っていない小節のスロットを踏み潰すまでの余裕」**だけ。
///
/// # なぜ 4 本なのか（2 本ではない）
/// DAW の先読みは 1 小節先まで（`daw/src/playback/live_cache.rs`）。演奏ループが
/// サーバーのサンプルクロックより `D` 小節ぶん先行しているとき、踏み潰しが起きるのは
/// **`D >= SLOT_COUNT`** のとき。つまり吸収できる先行は `SLOT_COUNT - 1` 小節ぶん。
///
/// 実測（`clap-mml-render-tui` の `docs/adr/0012-live-clock-drift-is-absorbed-not-eliminated.md`）では、
/// 1 小節目の state load 中にクロックが止まるせいで **2.7 秒＝1.3 小節（BPM113）**
/// 先行していた。2 本では余裕が 1 小節しか無く、**実際に踏み潰して違う小節が鳴った。**
/// 4 本なら余裕は 3 小節（BPM113 で 6.4 秒）になる。
///
/// 値段はメモリだけ。1 本 1.54MB（4 秒ステレオ f32）なので、
/// **16 instance すべてを使っても 49MB → 98MB**（実測は
/// `docs/adr/0019-cache-player-slot-headroom.md`）。RT スレッドは `Arc` の clone しか
/// しないので、増やしても `process` の重さは変わらない。
///
/// **奇数に近い値にしないこと。** 小節 index `N` はスロット `N % SLOT_COUNT` へ載り、
/// note number は `60 + (N % SLOT_COUNT)`。60 は 4 の倍数なので
/// `slot_for_note(60 + s) == s` が成り立つ（[`slot_for_note`]）。
pub const SLOT_COUNT: usize = 4;

/// state の綴りのプレフィクス。`slot=1;C:\...\track2_meas3.wav` の形。
///
/// **サフィックス形（`...wav#1`）にはできない。** patch 文字列は
/// `Path::new(patch).extension() == "wav"` でプラグインへ routing されるので
/// （`core-lib/src/cache_wav.rs`）、末尾に何か足すと拡張子判定が壊れる。
const SLOT_PREFIX: &str = "slot=";

/// プレフィクスとパスの区切り。
const SLOT_SEPARATOR: char = ';';

/// note number からスロット index を決める。
///
/// **`note % SLOT_COUNT`。** DAW は小節 index `N` の note on に
/// `60 + (N % SLOT_COUNT)` を送り、同じ `N % SLOT_COUNT` のスロットへ
/// その小節の WAV を載せる。**60（C4）は [`SLOT_COUNT`] の倍数**なので
/// `slot_for_note(60 + s) == s` になる。
///
/// 剰余にしてあるのは、**範囲外の note で黙って無音にならないため**。
/// 音高を見ていなかった頃の呼び出し（常に note 60）はスロット 0 に落ちるので、
/// 後方互換もこれで取れる。CLAP note event が `Match::All`（音高指定なし）の
/// ときも呼び出し側でスロット 0 として扱う。
pub fn slot_for_note(note: u8) -> usize {
    note as usize % SLOT_COUNT
}

/// CLAP state 1 件が指示する内容。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateRequest {
    /// 空 state。全スロットを空にする。
    ClearAll,
    /// このスロットを空にする（`slot=1;` のようにパスが空の綴り）。
    Clear { slot: usize },
    /// このスロットへ WAV を載せる。
    Load { slot: usize, path: String },
}

/// CLAP state の文字列を解釈する。
///
/// 綴りの定義は `core-lib/src/cache_wav.rs` の module doc（単一ソース）を見ること。
///
/// スロット番号が範囲外・数値でない場合はエラーにする。黙って 0 へ落とすと
/// 「載せたはずの小節が鳴らない」が原因不明の無音になる。
pub fn parse_state(state: &str) -> Result<StateRequest, String> {
    let state = state.trim();
    if state.is_empty() {
        return Ok(StateRequest::ClearAll);
    }

    let Some(rest) = state.strip_prefix(SLOT_PREFIX) else {
        return Ok(StateRequest::Load {
            slot: 0,
            path: state.to_string(),
        });
    };
    let Some((number, path)) = rest.split_once(SLOT_SEPARATOR) else {
        return Err(format!("スロット指定に '{SLOT_SEPARATOR}' が無い: {state}"));
    };
    let slot: usize = number
        .trim()
        .parse()
        .map_err(|_| format!("スロット番号が数値でない: {state}"))?;
    if slot >= SLOT_COUNT {
        return Err(format!("スロット番号が範囲外（0..{SLOT_COUNT}）: {state}"));
    }

    let path = path.trim();
    if path.is_empty() {
        return Ok(StateRequest::Clear { slot });
    }
    Ok(StateRequest::Load {
        slot,
        path: path.to_string(),
    })
}

/// [`parse_state`] が読める綴りを組み立てる。プレフィクスの綴りをこの crate に閉じるための口。
pub fn slot_patch_state(slot: usize, path: &str) -> String {
    format!("{SLOT_PREFIX}{slot}{SLOT_SEPARATOR}{path}")
}

/// スロット指定を外して `(スロット指定が付いていたか, パス部分)` を返す。
///
/// **綴りが壊れていても失敗しない。** patch 文字列を拡張子で routing する側
/// （`core-lib/src/cache_wav.rs`）が、`slot=9;` や `slot=x;` のような壊れた綴りも
/// 「cache-player 宛て」として拾えるようにするための口。拾わないと Surge XT の
/// state file として扱われ、「スロット番号を間違えた」が「state のロードに失敗」という
/// 原因の遠いエラーになる。**壊れているかどうかを決めるのは [`parse_state`] だけ。**
pub fn split_slot_prefix(state: &str) -> (bool, &str) {
    let state = state.trim();
    let Some(rest) = state.strip_prefix(SLOT_PREFIX) else {
        return (false, state);
    };
    match rest.split_once(SLOT_SEPARATOR) {
        Some((_, path)) => (true, path.trim()),
        None => (true, ""),
    }
}

/// 固定長のスロット置き場。
///
/// RT スレッドは `Arc` の clone しかしない（確保も解放もしない）。
#[derive(Clone, Default)]
pub struct CacheSlots {
    slots: [Option<Arc<CacheBuffer>>; SLOT_COUNT],
}

impl CacheSlots {
    pub fn get(&self, slot: usize) -> Option<&Arc<CacheBuffer>> {
        self.slots.get(slot).and_then(Option::as_ref)
    }

    pub fn set(&mut self, slot: usize, buffer: Option<Arc<CacheBuffer>>) {
        if let Some(entry) = self.slots.get_mut(slot) {
            *entry = buffer;
        }
    }

    pub fn clear_all(&mut self) {
        for slot in self.slots.iter_mut() {
            *slot = None;
        }
    }

    /// 抱えている `Arc` を graveyard へ預けて空にする。
    ///
    /// RT スレッドが古いスロットを手放すときの口。**ここで drop すると、
    /// その `Arc` が最後の参照だったときに `process` の中で解放が走る。**
    pub fn bury_into(&mut self, graveyard: &mut BufferGraveyard) {
        for slot in self.slots.iter_mut() {
            if let Some(buffer) = slot.take() {
                graveyard.bury(buffer);
            }
        }
    }
}

#[cfg(test)]
mod tests;
