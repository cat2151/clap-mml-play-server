//! DAW 風モード
//!
//! 8 tracks × (0..=8 measures) の matrix
//!   measure 0 = 音色 (timbre) / track ごとの共通ヘッダ
//!   track   0 = テンポ (t120 など) → render 時に全小節の先頭にくっつける
//!
//! キー操作 (NORMAL):
//!   h/l    : 小節 (列) 移動
//!   j/k    : track (行) 移動
//!   i      : INSERT モード（現在セルを編集）
//!   p      : 演奏 / 停止 toggle
//!   r      : measure 0 にランダム音色を設定
//!   q / ESC: DAW モード終了 → TUI に戻る
//!
//! キー操作 (INSERT):
//!   ESC   : 確定 → NORMAL
//!   Enter : 確定 → 次の小節へ移動 → INSERT 継続
//!   ;     : 確定時にセミコロンで分割し、下の track に順に追加

mod ui;

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use tui_textarea::TextArea;

use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::patch_list::{collect_patches, to_relative};

/// track 数（固定）
pub const TRACKS: usize = 8;
/// 小節数（固定）。measure 0 = 音色列。measure 1..=MEASURES = 通常小節。
pub const MEASURES: usize = 8;
/// track 0 はグローバルヘッダ（テンポ等）専用。演奏 track は 1 から始まる。
const FIRST_PLAYABLE_TRACK: usize = 1;

const DAW_FILE: &str = "daw.txt";
const DAW_MML_DEBUG_FILE: &str = "daw_mml_debug.txt";

// ─── キャッシュ ───────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub(super) enum CacheState {
    Empty,   // MML が空
    Pending, // MML あり、レンダリング待ち or 実行中
    Ready,   // レンダリング済み
    Error,   // レンダリング失敗
}

#[derive(Clone)]
pub(super) struct CellCache {
    pub(super) state: CacheState,
}

impl CellCache {
    fn empty() -> Self {
        Self { state: CacheState::Empty }
    }
}

// ─── 演奏状態 ─────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub(super) enum DawPlayState {
    Idle,
    Playing,
}

// ─── 内部モード ───────────────────────────────────────────────

#[derive(PartialEq)]
pub(super) enum DawMode {
    Normal,
    Insert,
}

// ─── DawApp ───────────────────────────────────────────────────

pub struct DawApp {
    /// data[track][measure]: track 0..TRACKS, measure 0..=MEASURES
    pub(super) data: Vec<Vec<String>>,

    pub(super) cursor_track: usize,   // 0..TRACKS-1
    pub(super) cursor_measure: usize, // 0..=MEASURES  (0 = 音色列)

    pub(super) mode: DawMode,
    pub(super) textarea: TextArea<'static>,

    cfg: Arc<Config>,
    entry_ptr: usize, // *const PluginEntry as usize (main() に生存保証)

    /// セルごとのキャッシュ [track][measure]
    pub(super) cache: Arc<Mutex<Vec<Vec<CellCache>>>>,

    /// キャッシュワーカースレッドへのジョブチャネル: (track, measure, mml)
    /// シリアルな単一ワーカーで処理することでファイル書き込みの競合を防ぐ
    cache_tx: std::sync::mpsc::Sender<(usize, usize, String)>,

    pub(super) play_state: Arc<Mutex<DawPlayState>>,
}

impl DawApp {
    pub fn new(cfg: Arc<Config>, entry_ptr: usize) -> Self {
        let mut data = vec![vec![String::new(); MEASURES + 1]; TRACKS];
        // track 0 のデフォルトはテンポ設定
        data[0][0] = "t120".to_string();

        let cache = Arc::new(Mutex::new(
            vec![vec![CellCache::empty(); MEASURES + 1]; TRACKS],
        ));

        // シリアルなキャッシュワーカースレッドを起動する。
        // チャネルが送信側（cache_tx）を介してジョブを受け取り順次レンダリングすることで
        // ファイル書き込み（pass1_tokens.json 等）の競合と過剰スレッド生成を防ぐ。
        let (cache_tx, cache_rx) = std::sync::mpsc::channel::<(usize, usize, String)>();
        {
            let cache_worker = Arc::clone(&cache);
            let cfg_worker = Arc::clone(&cfg);
            std::thread::spawn(move || {
                // SAFETY: entry は main() のスタックに生存している
                let entry_ref: &PluginEntry = unsafe { &*(entry_ptr as *const PluginEntry) };
                let mut daw_cfg = (*cfg_worker).clone();
                daw_cfg.random_patch = false;

                for (track, measure, mml) in cache_rx {
                    match crate::pipeline::mml_render_for_cache(&mml, &daw_cfg, entry_ref) {
                        Ok(_) => {
                            cache_worker.lock().unwrap()[track][measure].state = CacheState::Ready;
                        }
                        Err(_) => {
                            cache_worker.lock().unwrap()[track][measure].state = CacheState::Error;
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
            cache,
            cache_tx,
            play_state: Arc::new(Mutex::new(DawPlayState::Idle)),
        };

        app.load();
        app
    }

    // ─── 保存 / 読み込み ──────────────────────────────────────

    fn load(&mut self) {
        if let Ok(content) = std::fs::read_to_string(DAW_FILE) {
            for (t, track_str) in content.split(';').enumerate() {
                if t >= TRACKS {
                    break;
                }
                for (m, cell) in track_str.split('\n').enumerate() {
                    if m > MEASURES {
                        break;
                    }
                    self.data[t][m] = cell.to_string();
                }
            }
        }
        self.sync_cache_states();
    }

    fn save(&self) {
        let tracks: Vec<String> = self
            .data
            .iter()
            .map(|track| track.join("\n"))
            .collect();
        let _ = std::fs::write(DAW_FILE, tracks.join(";"));
    }

    // ─── キャッシュ管理 ───────────────────────────────────────

    /// data の内容に合わせてキャッシュ状態を同期する（data 変更後に呼ぶ）
    fn sync_cache_states(&self) {
        let mut cache = self.cache.lock().unwrap();
        for t in 0..TRACKS {
            for m in 0..=MEASURES {
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
            cache[track][measure] = CellCache { state: CacheState::Pending };
        }
    }

    /// 指定セルのキャッシュジョブをワーカーキューに投入する
    fn kick_cache(&self, track: usize, measure: usize) {
        let mml = self.build_cell_mml(track, measure);
        if mml.trim().is_empty() {
            return;
        }
        // チャネルが既に閉じていれば送信は無視する（DawApp 終了後の残留呼び出しへの安全策）
        let _ = self.cache_tx.send((track, measure, mml));
    }

    /// Pending 状態のすべてのセルをワーカーキューに投入する
    fn kick_all_pending(&self) {
        let pending: Vec<(usize, usize)> = {
            let cache = self.cache.lock().unwrap();
            (0..TRACKS)
                .flat_map(|t| (0..=MEASURES).map(move |m| (t, m)))
                .filter(|&(t, m)| cache[t][m].state == CacheState::Pending)
                .collect()
        };
        for (t, m) in pending {
            self.kick_cache(t, m);
        }
    }

    // ─── MML 構築 ─────────────────────────────────────────────

    /// セル (track, measure) のレンダリング用 MML を構築する
    /// = track[t][0] (音色) + track0 全体 + track[t][m] (音符)
    /// 音色 JSON を先頭に置くことで extract_embedded_json が正しく解析できる
    fn build_cell_mml(&self, track: usize, measure: usize) -> String {
        let track0: String = (0..=MEASURES)
            .map(|m| self.data[0][m].trim())
            .collect::<Vec<_>>()
            .join("");
        let timbre = self.data[track][0].trim();
        let notes = self.data[track][measure].trim();
        format!("{}{}{}", timbre, track0, notes)
    }

    /// 全 track を結合したフル曲 MML を構築する（演奏用）
    /// track 0 はグローバルヘッダ（テンポ等）として各 track の先頭に付加するが、
    /// それ自体を独立した再生 track としては扱わない。
    /// 音色 JSON を先頭に置くことで extract_embedded_json が正しく解析できる
    fn build_full_mml(&self) -> String {
        let track0: String = (0..=MEASURES)
            .map(|m| self.data[0][m].trim())
            .collect::<Vec<_>>()
            .join("");

        let track_mmls: Vec<String> = (FIRST_PLAYABLE_TRACK..TRACKS)
            .filter_map(|t| {
                let timbre = self.data[t][0].trim();
                let notes: String = (1..=MEASURES)
                    .map(|m| self.data[t][m].trim())
                    .collect::<Vec<_>>()
                    .join("");
                if timbre.is_empty() && notes.is_empty() {
                    None
                } else {
                    Some(format!("{}{}{}", timbre, track0, notes))
                }
            })
            .collect();

        track_mmls.join(";")
    }

    // ─── 演奏 ─────────────────────────────────────────────────

    fn start_play(&self) {
        let full_mml = self.build_full_mml();
        if full_mml.trim().is_empty() {
            return;
        }

        // デバッグ用ファイルに組み立てた MML を出力する
        let _ = std::fs::write(DAW_MML_DEBUG_FILE, &full_mml);

        let play_state = Arc::clone(&self.play_state);
        let cfg = Arc::clone(&self.cfg);
        let entry_ptr = self.entry_ptr;

        *play_state.lock().unwrap() = DawPlayState::Playing;

        std::thread::spawn(move || {
            // SAFETY: entry は main() のスタックに生存している
            let entry_ref: &PluginEntry = unsafe { &*(entry_ptr as *const PluginEntry) };
            let mut daw_cfg = (*cfg).clone();
            daw_cfg.random_patch = false;

            loop {
                if *play_state.lock().unwrap() != DawPlayState::Playing {
                    break;
                }
                // mml_render_for_cache を使用することで patch_history.txt への追記を行わない
                match crate::pipeline::mml_render_for_cache(&full_mml, &daw_cfg, entry_ref) {
                    Ok(samples) => {
                        if *play_state.lock().unwrap() != DawPlayState::Playing {
                            break;
                        }
                        let _ = crate::pipeline::play_samples(
                            samples,
                            daw_cfg.sample_rate as u32,
                        );
                    }
                    Err(_) => break,
                }
            }

            *play_state.lock().unwrap() = DawPlayState::Idle;
        });
    }

    fn stop_play(&self) {
        *self.play_state.lock().unwrap() = DawPlayState::Idle;
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

    // ─── INSERT モード ────────────────────────────────────────

    fn start_insert(&mut self) {
        let mut ta = TextArea::default();
        for ch in self.data[self.cursor_track][self.cursor_measure].chars() {
            ta.insert_char(ch);
        }
        self.textarea = ta;
        self.mode = DawMode::Insert;
    }

    /// 編集内容を確定してキャッシュ更新・保存を行う
    fn commit_insert(&mut self) {
        let text = self.textarea.lines().join("");

        if text.contains(';') {
            // セミコロンで分割して下の track に順に追加
            for (i, part) in text.split(';').enumerate() {
                let t = self.cursor_track + i;
                if t >= TRACKS {
                    break;
                }
                self.data[t][self.cursor_measure] = part.to_string();
                self.invalidate_cell(t, self.cursor_measure);
                self.kick_cache(t, self.cursor_measure);
            }
        } else {
            self.data[self.cursor_track][self.cursor_measure] = text;
            self.invalidate_cell(self.cursor_track, self.cursor_measure);
            self.kick_cache(self.cursor_track, self.cursor_measure);
        }

        self.save();
    }

    // ─── キー処理 ─────────────────────────────────────────────

    fn handle_normal(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => return true,

            KeyCode::Char('h') | KeyCode::Left => {
                if self.cursor_measure > 0 {
                    self.cursor_measure -= 1;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.cursor_measure < MEASURES {
                    self.cursor_measure += 1;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.cursor_track + 1 < TRACKS {
                    self.cursor_track += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.cursor_track > 0 {
                    self.cursor_track -= 1;
                }
            }

            KeyCode::Char('i') => self.start_insert(),

            KeyCode::Char('p') => {
                let state = self.play_state.lock().unwrap().clone();
                if state == DawPlayState::Playing {
                    self.stop_play();
                } else {
                    self.start_play();
                }
            }

            KeyCode::Char('r') => {
                // measure 0 にランダム音色を設定
                if let Some(patch) = self.pick_random_patch_name() {
                    self.data[self.cursor_track][0] =
                        format!("{{\"Surge XT patch\": \"{}\"}}", patch);
                    self.invalidate_cell(self.cursor_track, 0);
                    self.kick_cache(self.cursor_track, 0);
                    self.save();
                }
            }

            _ => {}
        }
        false
    }

    fn handle_insert(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            KeyCode::Esc => {
                self.commit_insert();
                self.mode = DawMode::Normal;
            }
            KeyCode::Enter => {
                // 確定 → 次の小節へ → INSERT 継続
                self.commit_insert();
                if self.cursor_measure < MEASURES {
                    self.cursor_measure += 1;
                }
                self.start_insert();
            }
            _ => {
                self.textarea.input(key_event);
            }
        }
    }

    // ─── 描画 ─────────────────────────────────────────────────

    fn draw(&self, f: &mut Frame) {
        ui::draw(self, f);
    }

    // ─── メインループ ─────────────────────────────────────────

    /// TuiApp と同じ terminal を受け取って DAW モードを実行する。
    /// 終了時（q / ESC）は `Ok(())` を返して TuiApp の loop に戻る。
    pub fn run_with_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
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
                        break;
                    }

                    let should_quit = match self.mode {
                        DawMode::Normal => self.handle_normal(key.code),
                        DawMode::Insert => {
                            self.handle_insert(key);
                            false
                        }
                    };

                    if should_quit {
                        break;
                    }
                }
            }
        }

        self.stop_play();
        Ok(())
    }
}
