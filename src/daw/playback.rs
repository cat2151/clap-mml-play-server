//! DawApp の演奏メソッド

use std::sync::Arc;

use clack_host::prelude::PluginEntry;

use super::{DawApp, DawPlayState, PlayPosition, DAW_MML_DEBUG_FILE};

impl DawApp {
    // ─── 演奏 ─────────────────────────────────────────────────

    pub(super) fn start_play(&self) {
        let measure_mmls = self.build_measure_mmls();
        if measure_mmls.iter().all(|m| m.trim().is_empty()) {
            return;
        }

        // cmrt/ ディレクトリを確保してからデバッグファイルを書き出す
        let _ = crate::pipeline::ensure_cmrt_dir();
        // デバッグ用ファイルに各小節の MML を出力する
        let _ = std::fs::write(DAW_MML_DEBUG_FILE, measure_mmls.join("\n---\n"));

        // play_measure_mmls と play_measure_samples を最新の値で更新してからスレッドに共有する
        *self.play_measure_mmls.lock().unwrap() = measure_mmls;
        *self.play_measure_samples.lock().unwrap() = self.measure_duration_samples();

        let play_state = Arc::clone(&self.play_state);
        let play_position = Arc::clone(&self.play_position);
        let play_measure_mmls = Arc::clone(&self.play_measure_mmls);
        let play_measure_samples = Arc::clone(&self.play_measure_samples);
        let render_lock = Arc::clone(&self.render_lock);
        let cfg = Arc::clone(&self.cfg);
        let entry_ptr = self.entry_ptr;

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

                for (measure_index, mml) in mmls.iter().enumerate() {
                    if *play_state.lock().unwrap() != DawPlayState::Playing {
                        break 'outer;
                    }

                    let samples = if mml.trim().is_empty() {
                        // 空小節: 1小節分の無音を再生して次の小節開始タイミングを保持する
                        vec![0.0f32; measure_samples]
                    } else {
                        // render_lock を取得してからレンダリングすることで、
                        // キャッシュワーカーと同時に cmrt/daw_cache.mid/wav を書き込まないようにする
                        let result = {
                            let _guard = render_lock.lock().unwrap();
                            // mml_render_for_cache を使用することで patch_history.txt への追記を行わない
                            crate::pipeline::mml_render_for_cache(mml, &daw_cfg, entry_ref)
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

                    if *play_state.lock().unwrap() != DawPlayState::Playing {
                        break 'outer;
                    }
                    // レンダリング完了後・再生直前に位置を記録することで、
                    // render 時間ではなく実際の再生時間に基づいたビート表示を実現する。
                    *play_position.lock().unwrap() = Some(PlayPosition {
                        measure_index,
                        measure_start: std::time::Instant::now(),
                    });
                    // 既存の Sink に追加して再生完了を待つ（OutputStream/Sink は使い回す）
                    let source = rodio::buffer::SamplesBuffer::new(2, sample_rate, samples);
                    sink.append(source);
                    sink.sleep_until_end();
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
        let cfg = Arc::clone(&self.cfg);
        let entry_ptr = self.entry_ptr;

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

            let result = {
                let _guard = render_lock.lock().unwrap();
                crate::pipeline::mml_render_for_cache(&mml, &daw_cfg, entry_ref)
            };

            // render が終わったら、まだ Preview セッションが有効なときだけ再生開始時刻を更新する。
            // stop や新しい演奏開始後に上書きしないようガードする。
            if *play_state.lock().unwrap() == DawPlayState::Preview {
                *play_position.lock().unwrap() = Some(PlayPosition {
                    measure_index,
                    measure_start: std::time::Instant::now(),
                });
            }

            if let Ok(mut samples) = result {
                if samples.len() < measure_samples {
                    samples.resize(measure_samples, 0.0);
                } else {
                    samples.truncate(measure_samples);
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

