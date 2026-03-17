//! DAW 風モード
//!
//! 9 tracks × (0..=8 measures) の matrix
//!   measure 0 = 音色 (timbre) / track ごとの共通ヘッダ
//!   track   0 = 拍子JSON + テンポ (例: `{"beat": "4/4"}t120`) → render 時に全小節の先頭にくっつける
//!
//! キー操作 (NORMAL):
//!   h/l    : 小節 (列) 移動
//!   j/k    : track (行) 移動
//!   H      : 先頭 track へ移動
//!   M      : 中央 track へ移動
//!   L      : 末尾 track へ移動
//!   i      : INSERT モード（現在セルを編集）
//!   p      : 演奏 / 停止 toggle
//!   r      : measure 0 にランダム音色を設定
//!   K      : ヘルプ表示
//!   q      : アプリ終了
//!   d / ESC: DAW モード終了 → TUI に戻る
//!
//! キー操作 (INSERT):
//!   ESC   : 確定 → NORMAL
//!   Enter : 確定 → 次の小節へ移動 → INSERT 継続
//!   ;     : 確定時にセミコロンで分割し、下の track に順に追加
//!
//! キー操作 (HELP):
//!   ESC   : キャンセル → NORMAL

mod input;
mod mml;
mod playback;
mod timing;
mod types;
mod ui;

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use tui_textarea::TextArea;

use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::patch_list::{collect_patches, to_relative};

// ─── 再エクスポート ───────────────────────────────────────────

pub(super) use types::{CacheState, CellCache, DawMode, DawNormalAction, DawPlayState, PlayPosition};
pub use types::DawExitReason;

// ─── 定数 ─────────────────────────────────────────────────────

/// track 数（固定）。track 0 = Tempo、track 1..=8 = 演奏 track。
pub const TRACKS: usize = 9;
/// 小節数（固定）。measure 0 = 音色列。measure 1..=MEASURES = 通常小節。
pub const MEASURES: usize = 8;
/// track 0 はグローバルヘッダ（テンポ等）専用。演奏 track は 1 から始まる。
const FIRST_PLAYABLE_TRACK: usize = 1;

/// インメモリキャッシュに保持するサンプル数の上限（ステレオ、インターリーブ）。
///
/// 2_000_000 サンプル / 2 ch = 1_000_000 samples per ch / 44100 Hz ≈ 22.7 秒 / 小節。
/// 4/4 拍子では BPM ≈ 4 * 60 / 22.7 ≈ 10.6 以上の小節がキャッシュ対象となる。
/// これを超えるサンプル数のセル（極端に低い BPM など）はキャッシュに保持せず、
/// 再生時にフォールバックレンダリングする。
/// ≈ 2_000_000 × 4 bytes ≈ 8 MB / cell。
pub(super) const MAX_CACHED_SAMPLES: usize = 2_000_000;

// ─── DawApp ───────────────────────────────────────────────────

pub struct DawApp {
    /// data[track][measure]: track 0..tracks, measure 0..=measures
    pub(super) data: Vec<Vec<String>>,

    pub(super) cursor_track: usize,   // 0..tracks-1
    pub(super) cursor_measure: usize, // 0..=measures  (0 = 音色列)

    pub(super) mode: DawMode,
    pub(super) textarea: TextArea<'static>,

    cfg: Arc<Config>,
    entry_ptr: usize, // *const PluginEntry as usize (main() に生存保証)

    /// config から読み込んだトラック数（track 0 = ヘッダ/テンポ、track 1.. = 演奏トラック）
    pub(super) tracks: usize,
    /// config から読み込んだ小節数（measure 0 = 音色列、measure 1.. = 通常小節）
    pub(super) measures: usize,

    /// セルごとのキャッシュ [track][measure]
    pub(super) cache: Arc<Mutex<Vec<Vec<CellCache>>>>,

    /// キャッシュワーカースレッドへのジョブチャネル: (track, measure, mml)
    /// シリアルな単一ワーカーで処理することでファイル書き込みの競合を防ぐ
    cache_tx: std::sync::mpsc::Sender<(usize, usize, String)>,

    /// `mml_render_for_cache` の排他実行ロック。
    /// キャッシュワーカーと再生スレッドが同時に daw_cache.mid/wav を書き込まないよう、
    /// `mml_render_for_cache` 呼び出し前に必ずこのロックを取得すること。
    render_lock: Arc<Mutex<()>>,

    pub(super) play_state: Arc<Mutex<DawPlayState>>,

    /// 再生中の小節・ビート位置（UI 描画に使用）
    pub(super) play_position: Arc<Mutex<Option<PlayPosition>>>,

    /// 再生スレッドと共有する各小節の MML ベクター（measures 要素, index i → meas i+1）。
    /// セル編集・ランダム音色変更のたびに更新されることで、
    /// play 中でも次ループ冒頭から新しい MML が反映される（hot reload）。
    play_measure_mmls: Arc<Mutex<Vec<String>>>,

    /// 再生スレッドと共有する 1 小節のステレオサンプル数。
    /// セル編集・ランダム音色変更のたびに `play_measure_mmls` と一緒に更新される。
    play_measure_samples: Arc<Mutex<usize>>,
}

impl DawApp {
    pub fn new(cfg: Arc<Config>, entry_ptr: usize) -> Self {
        let tracks = cfg.daw_tracks.clamp(2, 64);
        let measures = cfg.daw_measures.clamp(1, 64);
        let mut data = vec![vec![String::new(); measures + 1]; tracks];
        // track 0 のデフォルトは拍子指定 JSON + テンポ設定
        data[0][0] = r#"{"beat": "4/4"}t120"#.to_string();

        let cache = Arc::new(Mutex::new(
            vec![vec![CellCache::empty(); measures + 1]; tracks],
        ));

        // シリアルなキャッシュワーカースレッドを起動する。
        // チャネルが送信側（cache_tx）を介してジョブを受け取り順次レンダリングすることで
        // ファイル書き込み（clap-mml-render-tui/pass1_tokens.json 等）の競合と過剰スレッド生成を防ぐ。
        let (cache_tx, cache_rx) = std::sync::mpsc::channel::<(usize, usize, String)>();

        // `mml_render_for_cache` はキャッシュワーカーと再生スレッドの両方から呼ばれるため、
        // clap-mml-render-tui/daw/daw_cache.mid/wav への同時書き込みを防ぐ排他ロックを共有する。
        let render_lock: Arc<Mutex<()>> = Arc::new(Mutex::new(()));

        {
            let cache_worker = Arc::clone(&cache);
            let cfg_worker = Arc::clone(&cfg);
            let render_lock_worker = Arc::clone(&render_lock);
            std::thread::spawn(move || {
                // SAFETY: entry は main() のスタックに生存している
                let entry_ref: &PluginEntry = unsafe { &*(entry_ptr as *const PluginEntry) };
                let mut daw_cfg = (*cfg_worker).clone();
                daw_cfg.random_patch = false;

                for (track, measure, mml) in cache_rx {
                    let _guard = render_lock_worker.lock().unwrap();
                    match crate::pipeline::mml_render_for_cache(&mml, &daw_cfg, entry_ref) {
                        Ok(samples) => {
                            // 開発用: track/measure ごとに WAV ファイルを出力する
                            // measure 0 は音色/ヘッダセルであり演奏内容ではないためスキップ
                            let wav_ok = if measure > 0 {
                                if let Ok(daw_dir) = crate::pipeline::ensure_daw_dir() {
                                    let wav_path = daw_dir.join(format!("track{}_meas{}.wav", track, measure));
                                    crate::pipeline::write_wav(
                                        &samples,
                                        daw_cfg.sample_rate as u32,
                                        &wav_path,
                                    ).is_ok()
                                } else {
                                    false
                                }
                            } else {
                                true
                            };
                            // WAV 書き出し失敗はデバッグ出力の問題であり、レンダリング自体は成功している。
                            // そのため WAV 失敗時は Error としてユーザーに通知する。
                            let new_state = if wav_ok { CacheState::Ready } else { CacheState::Error };
                            let mut cache = cache_worker.lock().unwrap();
                            cache[track][measure].state = new_state;
                            // Ready かつサイズ上限以内のときのみサンプルをメモリに保持する。
                            // 上限超過（低 BPM 等）や WAV 失敗時はサンプルを保持しない。
                            if wav_ok && samples.len() <= MAX_CACHED_SAMPLES {
                                cache[track][measure].samples = Some(Arc::new(samples));
                            } else {
                                cache[track][measure].samples = None;
                            }
                        }
                        Err(_) => {
                            let mut cache = cache_worker.lock().unwrap();
                            cache[track][measure].state = CacheState::Error;
                            // エラー時は古いサンプルを保持しない（ステールデータの排除）
                            cache[track][measure].samples = None;
                        }
                    }
                }
            });
        }

        let mut app = Self {
            data,
            cursor_track: 0,
            cursor_measure: 0,
            mode: DawMode::Normal,
            textarea: TextArea::default(),
            cfg,
            entry_ptr,
            tracks,
            measures,
            cache,
            cache_tx,
            render_lock,
            play_state: Arc::new(Mutex::new(DawPlayState::Idle)),
            play_position: Arc::new(Mutex::new(None)),
            play_measure_mmls: Arc::new(Mutex::new(vec![String::new(); measures])),
            play_measure_samples: Arc::new(Mutex::new(0)),
        };

        app.load();
        app
    }

    // ─── 保存 / 読み込み ──────────────────────────────────────

    fn load(&mut self) {
        let path = crate::history::daw_file_path();
        let content = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok());
        if let Some(content) = content {
            for (t, track_str) in content.split(';').enumerate() {
                if t >= self.tracks {
                    break;
                }
                for (m, cell) in track_str.split('\n').enumerate() {
                    if m > self.measures {
                        break;
                    }
                    self.data[t][m] = cell.to_string();
                }
            }
        }
        self.sync_cache_states();
    }

    fn save(&self) {
        let Some(path) = crate::history::daw_file_path() else { return; };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tracks: Vec<String> = self
            .data
            .iter()
            .map(|track| track.join("\n"))
            .collect();
        let _ = std::fs::write(&path, tracks.join(";"));
    }

    // ─── キャッシュ管理 ───────────────────────────────────────

    /// data の内容に合わせてキャッシュ状態を同期する（data 変更後に呼ぶ）
    fn sync_cache_states(&self) {
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
    fn invalidate_cell(&self, track: usize, measure: usize) {
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
    fn kick_cache(&self, track: usize, measure: usize) {
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
    fn kick_all_pending(&self) {
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

    // ─── ランダム音色 ─────────────────────────────────────────

    fn pick_random_patch_name(&self) -> Option<String> {
        let dir = self.cfg.patches_dir.as_deref()?;
        let patches = collect_patches(dir).ok()?;
        if patches.is_empty() {
            return None;
        }
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0) as usize;
        let idx = ns % patches.len();
        Some(to_relative(dir, &patches[idx]))
    }

    // ─── 描画 ─────────────────────────────────────────────────

    fn draw(&self, f: &mut Frame) {
        ui::draw(self, f);
    }

    // ─── メインループ ─────────────────────────────────────────

    /// TuiApp と同じ terminal を受け取って DAW モードを実行する。
    /// 終了時は `DawExitReason` を返す:
    ///   - `ReturnToTui` : d / ESC キーで TUI に戻る
    ///   - `QuitApp`     : q キーまたは Ctrl+C でアプリを終了する
    pub fn run_with_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<DawExitReason> {
        // Pending セルのキャッシュ構築を開始
        self.kick_all_pending();

        loop {
            terminal.draw(|f| self.draw(f))?;

            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    use crossterm::event::KeyEventKind;
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        self.stop_play();
                        return Ok(DawExitReason::QuitApp);
                    }

                    match self.mode {
                        DawMode::Normal => match self.handle_normal(key.code) {
                            DawNormalAction::ReturnToTui => {
                                self.stop_play();
                                return Ok(DawExitReason::ReturnToTui);
                            }
                            DawNormalAction::QuitApp => {
                                self.stop_play();
                                return Ok(DawExitReason::QuitApp);
                            }
                            DawNormalAction::Continue => {}
                        },
                        DawMode::Insert => {
                            self.handle_insert(key);
                        }
                        DawMode::Help => {
                            self.handle_help(key.code);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

