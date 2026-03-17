//! DAW モードの型定義

use std::sync::Arc;

// ─── キャッシュ ───────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub enum CacheState {
    Empty,   // MML が空
    Pending, // MML あり、レンダリング待ち or 実行中
    Ready,   // レンダリング済み
    Error,   // レンダリング失敗
}

/// セルごとのレンダリングキャッシュ。
///
/// `samples` フィールドはメモリ内にステレオ PCM サンプルを保持する。
/// 低 BPM では 1 小節が非常に長くなり得るため（BPM=1, 4/4, 44100 Hz → ~21M サンプル ≈ 85 MB/cell）、
/// キャッシュワーカーはサイズが [`MAX_CACHED_SAMPLES`] を超えるセルのサンプルを保持しない
/// （その場合 `state` は `Ready` だが `samples` は `None` のまま残り、再生時にフォールバックレンダリングされる）。
///
/// [`MAX_CACHED_SAMPLES`]: super::MAX_CACHED_SAMPLES
#[derive(Clone)]
pub struct CellCache {
    pub(super) state: CacheState,
    /// レンダリング済みのステレオサンプル（Ready かつサイズ上限以内のときのみ Some）
    pub(super) samples: Option<Arc<Vec<f32>>>,
}

impl CellCache {
    pub(super) fn empty() -> Self {
        Self { state: CacheState::Empty, samples: None }
    }
}

// ─── 演奏状態 ─────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub enum DawPlayState {
    Idle,
    Playing,
    /// 一発プレビュー（ループなし）
    Preview,
}

/// 再生中の小節・ビート位置
#[derive(Clone)]
pub struct PlayPosition {
    /// 0-based 小節インデックス（表示は +1）
    pub measure_index: usize,
    /// この小節の再生開始時刻
    pub measure_start: std::time::Instant,
}

// ─── 内部モード ───────────────────────────────────────────────

#[derive(PartialEq)]
pub enum DawMode {
    Normal,
    Insert,
    Help,
}

/// handle_normal の戻り値
pub enum DawNormalAction {
    Continue,
    ReturnToTui,
    QuitApp,
}

/// DAW モード終了後の TUI への通知
pub enum DawExitReason {
    /// d / ESC キーで TUI に戻る
    ReturnToTui,
    /// q キーまたは Ctrl+C でアプリを終了する
    QuitApp,
}
