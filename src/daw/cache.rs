//! DAW セルキャッシュの管理

use super::{CacheState, CellCache, DawApp};

impl DawApp {
    // ─── キャッシュ管理 ───────────────────────────────────────

    /// data の内容に合わせてキャッシュ状態を同期する（data 変更後に呼ぶ）
    pub(super) fn sync_cache_states(&self) {
        let mut cache = self.cache.lock().unwrap();
        for t in 0..self.tracks {
            for m in 0..=self.measures {
                if self.data[t][m].trim().is_empty() {
                    cache[t][m] = CellCache::empty();
                } else if cache[t][m].state == CacheState::Empty {
                    cache[t][m].state = CacheState::Pending;
                }
            }
        }
    }

    /// 指定セルのキャッシュを無効化して状態を更新する
    pub(super) fn invalidate_cell(&self, track: usize, measure: usize) {
        let mut cache = self.cache.lock().unwrap();
        if self.data[track][measure].trim().is_empty() {
            cache[track][measure] = CellCache::empty();
        } else {
            cache[track][measure] = CellCache { state: CacheState::Pending, samples: None };
        }
    }

    /// 指定セルのキャッシュジョブをワーカーキューに投入する
    ///
    /// セル自身の内容（`data[track][measure]`）が空のときはジョブを投入しない。
    /// 以前は `build_cell_mml()` の結果（track0 を含む結合 MML）で空判定していたため、
    /// セルの内容を消去しても `●` インジケータが消えないバグがあった（issue #69 参照）。
    pub(super) fn kick_cache(&self, track: usize, measure: usize) {
        // セル自身の内容が空なら投入しない（track0 含む結合 MML で判定しない）
        if self.data[track][measure].trim().is_empty() {
            return;
        }
        let mml = self.build_cell_mml(track, measure);
        // チャネルが既に閉じていれば送信は無視する（DawApp 終了後の残留呼び出しへの安全策）
        let _ = self.cache_tx.send((track, measure, mml));
    }

    /// 依存セルを一括で無効化してキャッシュジョブを投入する。
    ///
    /// `build_cell_mml(t, m)` はセル自身の内容に加え track0（グローバルヘッダ）と
    /// 音色セル `data[t][0]` を参照するため、それらが変化した際に依存セルも再レンダリングが必要。
    ///
    /// - track == 0（グローバルヘッダ変更）→ 全演奏トラック（1..tracks）の全小節を再キャッシュ
    /// - measure == 0 かつ track > 0（音色変更）→ 同トラックの全小節（1..=measures）を再キャッシュ
    /// - それ以外 → 追加の依存セルなし（呼び出し元が個別に処理済み）
    pub(super) fn invalidate_and_kick_dependent_cells(&self, track: usize, measure: usize) {
        if track == 0 {
            // track0 セル変更: 全演奏トラックの全小節が影響を受ける
            {
                let mut cache = self.cache.lock().unwrap();
                for t in 1..self.tracks {
                    for m in 1..=self.measures {
                        if self.data[t][m].trim().is_empty() {
                            cache[t][m] = CellCache::empty();
                        } else {
                            cache[t][m] = CellCache { state: CacheState::Pending, samples: None };
                        }
                    }
                }
            }
            for t in 1..self.tracks {
                for m in 1..=self.measures {
                    self.kick_cache(t, m);
                }
            }
        } else if measure == 0 {
            // 音色セル（data[track][0]）変更: 同トラックの全小節が影響を受ける（issue #67 参照）
            {
                let mut cache = self.cache.lock().unwrap();
                for m in 1..=self.measures {
                    if self.data[track][m].trim().is_empty() {
                        cache[track][m] = CellCache::empty();
                    } else {
                        cache[track][m] = CellCache { state: CacheState::Pending, samples: None };
                    }
                }
            }
            for m in 1..=self.measures {
                self.kick_cache(track, m);
            }
        }
        // measure > 0 かつ track > 0 の場合は依存セルなし
    }

    /// Pending 状態のすべてのセルをワーカーキューに投入する
    pub(super) fn kick_all_pending(&self) {
        let pending: Vec<(usize, usize)> = {
            let cache = self.cache.lock().unwrap();
            (0..self.tracks)
                .flat_map(|t| (0..=self.measures).map(move |m| (t, m)))
                .filter(|&(t, m)| cache[t][m].state == CacheState::Pending)
                .collect()
        };
        for (t, m) in pending {
            self.kick_cache(t, m);
        }
    }
}
