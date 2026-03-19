//! DawApp の演奏メソッド

use std::sync::{Arc, Mutex};

use clack_host::prelude::PluginEntry;
use cmrt_core::{CoreConfig, ensure_daw_dir, mml_render_for_cache};

use super::{CacheState, CellCache, DawApp, DawPlayState, PlayPosition, FIRST_PLAYABLE_TRACK};

/// 末尾の空小節を除いた有効な小節数を計算する。
///
/// すべての小節が空の場合は `None` を返す。
/// これにより meas3-8 が空のときは meas1-2 だけをループする（issue #68）。
pub(super) fn effective_measure_count(mmls: &[String]) -> Option<usize> {
    mmls.iter().rposition(|m| !m.trim().is_empty()).map(|idx| idx + 1)
}

/// キャッシュ済みのサンプルをミックスして返す。
///
/// 指定小節（`measure`、1始まり）のすべての playable track（`FIRST_PLAYABLE_TRACK..tracks`）の
/// キャッシュを調べ、合算したサンプルを返す。
/// いずれかの playable track が `Ready` でない（Pending / Error）場合は `None` を返し、
/// 呼び出し元はフレッシュレンダリングにフォールバックすること。
/// 全 playable track が `Empty` の場合は無音（ゼロ埋め）を返す。
/// 結果は `measure_samples` 長に正確に揃えて返す（超過分は切り捨て、不足分はゼロ埋め済み）。
fn try_get_cached_samples(
    cache: &Arc<Mutex<Vec<Vec<CellCache>>>>,
    measure: usize,
    measure_samples: usize,
    tracks: usize,
) -> Option<Vec<f32>> {
    // ロック下では Arc ハンドルの収集のみ行い、ミックス処理はロック外で実施する。
    // これによりキャッシュワーカーや UI スレッドとのロック競合を最小化する。
    let track_samples: Option<Vec<Option<Arc<Vec<f32>>>>> = {
        let cache = cache.lock().unwrap();
        let mut result = Vec::with_capacity(tracks - FIRST_PLAYABLE_TRACK);
        for t in FIRST_PLAYABLE_TRACK..tracks {
            match cache[t][measure].state {
                CacheState::Empty => {
                    result.push(None); // 空トラック
                }
                CacheState::Ready => {
                    // samples が None の場合（サイズ上限超過等）もフォールバック
                    let arc = cache[t][measure].samples.clone();
                    if arc.is_none() {
                        return None;
                    }
                    result.push(arc);
                }
                _ => {
                    // Pending または Error → キャッシュ未完成、フォールバックが必要
                    return None;
                }
            }
        }
        Some(result)
    };

    let track_samples = track_samples?;

    // ロック外でミックス処理を行う
    // 最初からゼロ埋め済みのバッファを使うことで measure_samples を超える書き込みを防ぐ
    let mut mixed = vec![0.0f32; measure_samples];
    let mut any_ready = false;

    for arc_opt in &track_samples {
        if let Some(s) = arc_opt {
            any_ready = true;
            let n = s.len().min(measure_samples);
            for i in 0..n {
                mixed[i] += s[i];
            }
        }
    }

    if !any_ready {
        // すべての playable track が Empty → 空トラックのみの小節として無音を返す
        return Some(mixed);
    }

    Some(mixed)
}

impl DawApp {
    // ─── 演奏 ─────────────────────────────────────────────────

    pub(super) fn start_play(&self) {
        let measure_mmls = self.build_measure_mmls();
        if measure_mmls.iter().all(|m| m.trim().is_empty()) {
            return;
        }

        // daw/ ディレクトリを確保してからデバッグファイルを書き出す
        if let Ok(daw_dir) = ensure_daw_dir() {
            let debug_file = daw_dir.join("daw_mml_debug.txt");
            let _ = std::fs::write(&debug_file, measure_mmls.join("\n---\n"));
        }

        // play_measure_mmls と play_measure_samples を最新の値で更新してからスレッドに共有する
        *self.play_measure_mmls.lock().unwrap() = measure_mmls;
        *self.play_measure_samples.lock().unwrap() = self.measure_duration_samples();

        let play_state = Arc::clone(&self.play_state);
        let play_position = Arc::clone(&self.play_position);
        let play_measure_mmls = Arc::clone(&self.play_measure_mmls);
        let play_measure_samples = Arc::clone(&self.play_measure_samples);
        let render_lock = Arc::clone(&self.render_lock);
        let cache = Arc::clone(&self.cache);
        let cfg = Arc::clone(&self.cfg);
        let entry_ptr = self.entry_ptr;
        let tracks = self.tracks;

        *play_state.lock().unwrap() = DawPlayState::Playing;

        std::thread::spawn(move || {
            // SAFETY: entry は main() のスタックに生存している
            let entry_ref: &PluginEntry = unsafe { &*(entry_ptr as *const PluginEntry) };
            let mut daw_cfg = (*cfg).clone();
            daw_cfg.random_patch = false;
            let sample_rate = daw_cfg.sample_rate as u32;

        // OutputStream と Sink をスレッドに 1 つだけ作成し、小節をまたいで再利用する。
        // これにより小節ごとのオーディオ初期化オーバーヘッドとグリッチを防ぐ。
        let Ok((_stream, stream_handle)) = rodio::OutputStream::try_default() else {
            // Audio init failed: only reset to Idle if we are still the active Playing session.
            let mut state = play_state.lock().unwrap();
            if *state == DawPlayState::Playing {
                *state = DawPlayState::Idle;
                drop(state);
                *play_position.lock().unwrap() = None;
            }
            return;
        };
        let Ok(sink) = rodio::Sink::try_new(&stream_handle) else {
            let mut state = play_state.lock().unwrap();
            if *state == DawPlayState::Playing {
                *state = DawPlayState::Idle;
                drop(state);
                *play_position.lock().unwrap() = None;
            }
            return;
        };

            'outer: loop {
                if *play_state.lock().unwrap() != DawPlayState::Playing {
                    break;
                }
                // ループの先頭で毎回 play_measure_mmls と play_measure_samples を読み取ることで、
                // セル編集・音色変更を次ループから即座に反映する（hot reload）
                let mmls = play_measure_mmls.lock().unwrap().clone();
                let measure_samples = *play_measure_samples.lock().unwrap();

                // 末尾の空小節をスキップ: 有効な最後の小節までをループ対象とする。
                // これにより meas3-8 が空のときは meas1-2 だけをループする（issue #68）。
                let effective_count = match effective_measure_count(&mmls) {
                    Some(n) => n,
                    None => break 'outer,
                };

                // 全有効小節のサンプルを事前収集する。
                // シームレス再生のため、全サンプルをまとめて Sink にキューイングしてから
                // 時間ベースで位置を更新する（issue #68）。
                let mut all_samples: Vec<(usize, Vec<f32>)> = Vec::with_capacity(effective_count);
                for measure_index in 0..effective_count {
                    if *play_state.lock().unwrap() != DawPlayState::Playing {
                        break 'outer;
                    }

                    let mml = &mmls[measure_index];
                    let samples = if mml.trim().is_empty() {
                        // 中間の空小節は無音で維持（前後の小節とのタイミングを保持）
                        vec![0.0f32; measure_samples]
                    } else if let Some(cached) = try_get_cached_samples(&cache, measure_index + 1, measure_samples, tracks) {
                        // キャッシュヒット: 事前レンダリング済みサンプルをそのまま使用
                        cached
                    } else {
                        // キャッシュミス: レンダリングにフォールバック
                        // render_lock を取得してからレンダリングすることで、
                        // キャッシュワーカーと同時に clap-mml-render-tui/daw/daw_cache.mid/wav を書き込まないようにする
                        let result = {
                            let _guard = render_lock.lock().unwrap();
                            // mml_render_for_cache を使用することで patch_history.txt への追記を行わない
                            let core_cfg = CoreConfig::from(&daw_cfg);
                            mml_render_for_cache(mml, &core_cfg, entry_ref)
                        };
                        match result {
                            Ok(mut s) => {
                                // 設定された拍子・テンポに基づく 1 小節の長さに正確に pad / truncate する
                                if s.len() < measure_samples {
                                    s.resize(measure_samples, 0.0);
                                } else {
                                    s.truncate(measure_samples);
                                }
                                s
                            }
                            Err(_) => break 'outer,
                        }
                    };
                    all_samples.push((measure_index, samples));
                }

                if all_samples.is_empty() {
                    break 'outer;
                }

                // インデックスとバッファを分離する。
                // バッファは clone せず所有権ごと Sink に移動することでメモリコピーを回避する。
                let (measure_indices, sample_bufs): (Vec<usize>, Vec<Vec<f32>>) =
                    all_samples.into_iter().unzip();

                // measure_samples はステレオインターリーブ（L/R 各 1 サンプル = 2 要素）のため
                // 実時間 = measure_samples / (sample_rate * 2) となる。
                let measure_duration_secs =
                    measure_samples as f64 / (sample_rate as f64 * 2.0);

                // 全サンプルを Sink にまとめてキューイングしてシームレス再生を実現する（issue #68）。
                // loop_start は最初の append 直前に記録することで、実際のオーディオ開始と
                // できる限り近いタイムスタンプを得て位置推定の精度を高める。
                let loop_start = std::time::Instant::now();
                for buf in sample_bufs {
                    sink.append(rodio::buffer::SamplesBuffer::new(2, sample_rate, buf));
                }

                // 時間ベースで各小節の再生開始位置を更新する。
                // 10 ms 粒度でポーリングすることで停止要求に素早く応答できる（issue #68）。
                for (i, measure_index) in measure_indices.iter().enumerate() {
                    let measure_start_target = loop_start
                        + std::time::Duration::from_secs_f64(
                            i as f64 * measure_duration_secs,
                        );
                    // この小節の期待開始時刻まで待機（停止チェック付き）
                    loop {
                        if std::time::Instant::now() >= measure_start_target {
                            break;
                        }
                        if *play_state.lock().unwrap() != DawPlayState::Playing {
                            break 'outer;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    if *play_state.lock().unwrap() != DawPlayState::Playing {
                        break 'outer;
                    }
                    *play_position.lock().unwrap() = Some(PlayPosition {
                        measure_index: *measure_index,
                        measure_start: measure_start_target,
                    });
                }

                // 最後の小節の再生完了を 10 ms 粒度でポーリング待機する。
                // sink.sleep_until_end() は停止要求を検出できないためポーリングを使用する。
                loop {
                    if sink.empty() {
                        break;
                    }
                    if *play_state.lock().unwrap() != DawPlayState::Playing {
                        break 'outer;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }

            // Only reset to Idle if we are still the active Playing session.
            // An unconditional write would clobber a newer session started after stop.
            let mut state = play_state.lock().unwrap();
            if *state == DawPlayState::Playing {
                *state = DawPlayState::Idle;
                drop(state);
                *play_position.lock().unwrap() = None;
            }
        });
    }

    /// 指定された小節を一度だけ再生するプレビュー（ループなし）
    pub(super) fn start_preview(&self, measure_index: usize) {
        let mmls = self.build_measure_mmls();
        let mml = mmls.get(measure_index).cloned().unwrap_or_default();
        if mml.trim().is_empty() {
            return;
        }

        let measure_samples = self.measure_duration_samples();
        let play_state = Arc::clone(&self.play_state);
        let play_position = Arc::clone(&self.play_position);
        let render_lock = Arc::clone(&self.render_lock);
        let cache = Arc::clone(&self.cache);
        let cfg = Arc::clone(&self.cfg);
        let entry_ptr = self.entry_ptr;
        let tracks = self.tracks;

        *play_state.lock().unwrap() = DawPlayState::Preview;

        std::thread::spawn(move || {
            // SAFETY: entry は main() のスタックに生存している
            let entry_ref: &PluginEntry = unsafe { &*(entry_ptr as *const PluginEntry) };
            let mut daw_cfg = (*cfg).clone();
            daw_cfg.random_patch = false;
            let sample_rate = daw_cfg.sample_rate as u32;

            let Ok((_stream, stream_handle)) = rodio::OutputStream::try_default() else {
                // Audio init failed: only reset to Idle if we are still the active Preview session.
                let mut state = play_state.lock().unwrap();
                if *state == DawPlayState::Preview {
                    *state = DawPlayState::Idle;
                    drop(state);
                    *play_position.lock().unwrap() = None;
                }
                return;
            };
            let Ok(sink) = rodio::Sink::try_new(&stream_handle) else {
                let mut state = play_state.lock().unwrap();
                if *state == DawPlayState::Preview {
                    *state = DawPlayState::Idle;
                    drop(state);
                    *play_position.lock().unwrap() = None;
                }
                return;
            };

            // キャッシュヒット時は即時再生、ミス時はレンダリングにフォールバック
            let samples_opt = if let Some(cached) = try_get_cached_samples(&cache, measure_index + 1, measure_samples, tracks) {
                Some(cached)
            } else {
                let result = {
                    let _guard = render_lock.lock().unwrap();
                    let core_cfg = CoreConfig::from(&daw_cfg);
                    mml_render_for_cache(&mml, &core_cfg, entry_ref)
                };
                result.ok().map(|mut s| {
                    if s.len() < measure_samples {
                        s.resize(measure_samples, 0.0);
                    } else {
                        s.truncate(measure_samples);
                    }
                    s
                })
            };

            if let Some(samples) = samples_opt {
                // サンプル取得成功後、まだ Preview セッションが有効なときだけ再生開始時刻を更新する。
                // stop や新しい演奏開始後に上書きしないようガードする。
                // レンダリング失敗時は play_position を更新せず、UI に再生中と表示させない。
                if *play_state.lock().unwrap() == DawPlayState::Preview {
                    *play_position.lock().unwrap() = Some(PlayPosition {
                        measure_index,
                        measure_start: std::time::Instant::now(),
                    });
                }
                // Preview 中に stop された場合は再生しない
                if *play_state.lock().unwrap() == DawPlayState::Preview {
                    let source = rodio::buffer::SamplesBuffer::new(2, sample_rate, samples);
                    sink.append(source);
                    sink.sleep_until_end();
                }
            }

            // Only reset to Idle if we are still the active Preview session.
            // An unconditional write would clobber a newer session started after stop.
            let mut state = play_state.lock().unwrap();
            if *state == DawPlayState::Preview {
                *state = DawPlayState::Idle;
                drop(state);
                *play_position.lock().unwrap() = None;
            }
        });
    }

    pub(super) fn stop_play(&self) {
        *self.play_state.lock().unwrap() = DawPlayState::Idle;
        *self.play_position.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::effective_measure_count;
    use super::super::MEASURES;

    // ─── effective_measure_count ──────────────────────────────────

    #[test]
    fn effective_measure_count_all_empty_returns_none() {
        let mmls = vec!["".to_string(); MEASURES];
        assert_eq!(effective_measure_count(&mmls), None);
    }

    #[test]
    fn effective_measure_count_skips_trailing_empty_measures() {
        // meas1=cccccccc, meas2=ffffffff, meas3-8 空 → 有効小節数=2（issue #68）
        let mut mmls = vec!["".to_string(); MEASURES];
        mmls[0] = "cccccccc".to_string();
        mmls[1] = "ffffffff".to_string();
        assert_eq!(effective_measure_count(&mmls), Some(2));
    }

    #[test]
    fn effective_measure_count_includes_internal_empty_measures() {
        // meas1 非空、meas2 空（中間）、meas3 非空、meas4-8 空 → 有効小節数=3
        let mut mmls = vec!["".to_string(); MEASURES];
        mmls[0] = "cde".to_string();
        mmls[2] = "fga".to_string();
        assert_eq!(effective_measure_count(&mmls), Some(3));
    }

    #[test]
    fn effective_measure_count_single_non_empty_measure() {
        let mut mmls = vec!["".to_string(); MEASURES];
        mmls[0] = "c".to_string();
        assert_eq!(effective_measure_count(&mmls), Some(1));
    }

    #[test]
    fn effective_measure_count_all_measures_non_empty() {
        let mmls: Vec<String> = (0..MEASURES).map(|i| format!("c{}", i)).collect();
        assert_eq!(effective_measure_count(&mmls), Some(MEASURES));
    }

    #[test]
    fn effective_measure_count_whitespace_only_treated_as_empty() {
        let mut mmls = vec!["".to_string(); MEASURES];
        mmls[0] = "cde".to_string();
        mmls[1] = "   ".to_string(); // whitespace-only → treated as empty (trailing)
        assert_eq!(effective_measure_count(&mmls), Some(1));
    }
}
