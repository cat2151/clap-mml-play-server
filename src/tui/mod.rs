//! vim 風 TUI
//!
//! モード:
//!   NORMAL : j/k で行移動、H/M/L で先頭/中央/末尾行へ移動、i/o で INSERT、t で音色選択、Enter/Space で再生、q で終了
//!   INSERT : tui-textarea で編集
//!            ESC   → 確定 → NORMAL（再生開始）
//!            Enter → 確定 → 次行に新規行挿入 → INSERT 継続
//!   PATCHSELECT : インクリメンタルサーチで音色を選択
//!            文字入力: フィルタ（space=AND条件）
//!            ↑↓:リスト移動  Enter:現在行の先頭にJSONで挿入（上書き）  ESC:キャンセル
//!   HELP : K で表示、ESC でキャンセル

mod input;
mod ui;

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::ListState, Frame, Terminal};
use tui_textarea::TextArea;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

/// audio_cache の最大エントリ数。超過時はキャッシュ全体をクリアしてから挿入する。
const AUDIO_CACHE_MAX_ENTRIES: usize = 64;

use crate::config::Config;

/// クエリ文字列（空白区切りでAND条件）でパッチリストをフィルタする。
/// `all` は (表示名, 小文字化済み表示名) のペアであること（起動時に一度だけ計算）。
fn filter_patches(all: &[(String, String)], query: &str) -> Vec<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    if terms.is_empty() {
        return all.iter().map(|(orig, _)| orig.clone()).collect();
    }
    all.iter()
        .filter(|(_, lower)| terms.iter().all(|t| lower.contains(t.as_str())))
        .map(|(orig, _)| orig.clone())
        .collect()
}

/// キャッシュからサンプルを取得する。`random_patch` が true の場合は常に `None` を返す。
fn resolve_cached_samples(
    cache: &HashMap<String, Vec<f32>>,
    mml: &str,
    random_patch: bool,
) -> Option<Vec<f32>> {
    if random_patch {
        None
    } else {
        cache.get(mml).cloned()
    }
}

/// キャッシュにサンプルを挿入する。上限に達した場合はキャッシュ全体をクリアしてから挿入する。
/// `random_patch` が true の場合は何もしない。
///
/// 呼び出し元は `audio_cache` のロックを保持した状態で `&mut HashMap` を渡すこと。
/// この関数自体は非同期に呼び出されないため、len 確認と insert は事実上アトミックである。
fn try_insert_cache(
    cache: &mut HashMap<String, Vec<f32>>,
    mml: String,
    samples: Vec<f32>,
    random_patch: bool,
) {
    if random_patch {
        return;
    }
    if cache.len() >= AUDIO_CACHE_MAX_ENTRIES && !cache.contains_key(&mml) {
        cache.clear();
    }
    cache.insert(mml, samples);
}

/// バックグラウンドパッチ読み込みの状態
enum PatchLoadState {
    Loading,
    Ready(Vec<(String, String)>), // (表示名, 小文字化済み表示名)
    Err(String),
}

#[derive(PartialEq)]
pub(super) enum Mode {
    Normal,
    Insert,
    PatchSelect,
    Help,
}

/// handle_normal の戻り値
enum NormalAction {
    Continue,
    Quit,
    LaunchDaw,
}

#[derive(Clone, PartialEq)]
pub(super) enum PlayState {
    Idle,
    Running(String),  // レンダリング中
    Playing(String),  // 演奏中
    Done(String),
    Err(String),
}

pub struct TuiApp<'a> {
    pub(super) mode: Mode,
    pub(super) lines: Vec<String>,
    pub(super) cursor: usize,
    pub(super) list_state: ListState,
    pub(super) textarea: TextArea<'a>,
    cfg: Arc<Config>,
    entry_ptr: usize, // *const PluginEntry as usize (main() に生存保証)
    pub(super) play_state: Arc<Mutex<PlayState>>,
    /// MML文字列 → レンダリング済みサンプルのキャッシュ（random_patchモード時は使用しない）
    pub(super) audio_cache: Arc<Mutex<HashMap<String, Vec<f32>>>>,
    // 音色選択モード用
    /// バックグラウンドスレッドが収集したパッチリストの状態
    patch_load_state: Arc<Mutex<PatchLoadState>>,
    /// PatchSelect 起動時にスナップショットした (表示名, 小文字化済み) ペアのリスト
    pub(super) patch_all: Vec<(String, String)>,
    pub(super) patch_query: String,          // 検索クエリ
    pub(super) patch_filtered: Vec<String>,  // フィルタ結果（表示名のみ）
    pub(super) patch_cursor: usize,          // フィルタ結果内のカーソル位置
    pub(super) patch_list_state: ListState,  // 音色選択リスト描画用
    /// バックグラウンドのアップデートチェックがtrueにセットしたらアップデートを実行
    pub update_available: Arc<AtomicBool>,
    /// 終了時 DAW モードだったかどうか（history.json に保存・復元する）
    pub(super) is_daw_mode: bool,
}

impl<'a> TuiApp<'a> {
    pub fn new(cfg: &'a Config, entry: &'a PluginEntry) -> Self {
        let cfg_arc = Arc::new(cfg.clone());

        // パッチリストはバックグラウンドスレッドで収集する。
        // 起動時の同期スキャンによる遅延を避けるため。
        let patch_load_state: Arc<Mutex<PatchLoadState>> =
            Arc::new(Mutex::new(PatchLoadState::Loading));
        {
            let state_bg = Arc::clone(&patch_load_state);
            let patches_dir = cfg.patches_dir.clone();
            std::thread::spawn(move || {
                match patches_dir {
                    None => {
                        *state_bg.lock().unwrap() = PatchLoadState::Ready(Vec::new());
                    }
                    Some(dir) => {
                        match crate::patch_list::collect_patches(&dir) {
                            Ok(paths) => {
                                let pairs: Vec<(String, String)> = paths
                                    .into_iter()
                                    .map(|p| {
                                        let rel = crate::patch_list::to_relative(&dir, &p);
                                        let lower = rel.to_lowercase();
                                        (rel, lower)
                                    })
                                    .collect();
                                *state_bg.lock().unwrap() = PatchLoadState::Ready(pairs);
                            }
                            Err(e) => {
                                *state_bg.lock().unwrap() = PatchLoadState::Err(e.to_string());
                            }
                        }
                    }
                }
            });
        }

        // `lines` は常に1行以上を保持する（不変条件）。
        // load_session_state() は lines が空でないことを保証している。
        let crate::history::SessionState { cursor, lines, is_daw_mode } = crate::history::load_session_state();
        let initial_cursor = cursor.min(lines.len() - 1);
        let mut list_state = ListState::default();
        list_state.select(Some(initial_cursor));

        Self {
            mode: Mode::Normal,
            lines,
            cursor: initial_cursor,
            list_state,
            textarea: TextArea::default(),
            cfg: cfg_arc,
            entry_ptr: entry as *const PluginEntry as usize,
            play_state: Arc::new(Mutex::new(PlayState::Idle)),
            audio_cache: Arc::new(Mutex::new(HashMap::new())),
            patch_load_state,
            patch_all: Vec::new(),
            patch_query: String::new(),
            patch_filtered: Vec::new(),
            patch_cursor: 0,
            patch_list_state: ListState::default(),
            update_available: Arc::new(AtomicBool::new(false)),
            is_daw_mode,
        }
    }

    fn kick_play(&self, mml: String) {
        let cfg = Arc::clone(&self.cfg);
        let state = Arc::clone(&self.play_state);
        let cache = Arc::clone(&self.audio_cache);
        let entry_ptr = self.entry_ptr;

        // キャッシュを確認（random_patchモード時はキャッシュを使用しない）
        let cached_samples = resolve_cached_samples(&cache.lock().unwrap(), &mml, cfg.random_patch);

        if let Some(samples) = cached_samples {
            // キャッシュヒット: レンダリングをスキップして即時再生
            let msg = format!("(cached) | {}", mml);
            *state.lock().unwrap() = PlayState::Playing(msg.clone());

            std::thread::spawn(move || {
                let play_result = crate::pipeline::play_samples(samples, cfg.sample_rate as u32);

                *state.lock().unwrap() = match play_result {
                    Ok(_)  => PlayState::Done(msg),
                    Err(e) => PlayState::Err(format!("エラー: {}", e)),
                };
            });
        } else {
            // キャッシュミス: レンダリングが必要
            *state.lock().unwrap() = PlayState::Running(mml.clone());

            std::thread::spawn(move || {
                // SAFETY: entry は main() のスタックに生存している
                let entry_ref: &PluginEntry = unsafe { &*(entry_ptr as *const PluginEntry) };

                // レンダリング
                let render_result = crate::pipeline::mml_render(&mml, &cfg, entry_ref);

                match render_result {
                    Err(e) => {
                        *state.lock().unwrap() = PlayState::Err(format!("エラー: {}", e));
                    }
                    Ok((samples, patch_name)) => {
                        // キャッシュに保存（random_patchモード時はキャッシュしない、上限超過時はクリア）
                        try_insert_cache(
                            &mut cache.lock().unwrap(),
                            mml.clone(),
                            samples.clone(),
                            cfg.random_patch,
                        );

                        let msg = format!("{} | {}", patch_name, mml);
                        // 演奏中に切り替え
                        *state.lock().unwrap() = PlayState::Playing(msg.clone());

                        // 再生（ブロッキング）
                        let play_result = crate::pipeline::play_samples(samples, cfg.sample_rate as u32);

                        *state.lock().unwrap() = match play_result {
                            Ok(_)  => PlayState::Done(msg),
                            Err(e) => PlayState::Err(format!("エラー: {}", e)),
                        };
                    }
                }
            });
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        ui::draw(self, f);
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // 前回 DAW モードで終了していた場合は直接 DAW モードで起動する
        let mut quit_from_startup_daw = false;
        if self.is_daw_mode {
            let mut daw = crate::daw::DawApp::new(Arc::clone(&self.cfg), self.entry_ptr);
            match daw.run_with_terminal(&mut terminal)? {
                crate::daw::DawExitReason::ReturnToTui => {
                    self.is_daw_mode = false;
                }
                crate::daw::DawExitReason::QuitApp => {
                    quit_from_startup_daw = true;
                }
            }
        }

        loop {
            if quit_from_startup_daw {
                break;
            }
            terminal.draw(|f| self.draw(f))?;

            // アップデートが利用可能になったら自動的にループを抜けてアップデートを実行する
            if self.update_available.load(Ordering::Relaxed) && self.mode == Mode::Normal {
                break;
            }

            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    // Press のみ処理。Release/Repeat は無視（二重発火防止）
                    use crossterm::event::KeyEventKind;
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        break;
                    }
                    match self.mode {
                        Mode::Normal => {
                            match self.handle_normal(key.code) {
                                NormalAction::Quit => break,
                                NormalAction::LaunchDaw => {
                                    let mut daw = crate::daw::DawApp::new(
                                        Arc::clone(&self.cfg),
                                        self.entry_ptr,
                                    );
                                    match daw.run_with_terminal(&mut terminal)? {
                                        crate::daw::DawExitReason::ReturnToTui => {
                                            self.is_daw_mode = false;
                                        }
                                        crate::daw::DawExitReason::QuitApp => {
                                            self.is_daw_mode = true;
                                            break;
                                        }
                                    }
                                }
                                NormalAction::Continue => {}
                            }
                        }
                        Mode::Insert => self.handle_insert(key),
                        Mode::PatchSelect => self.handle_patch_select(key),
                        Mode::Help => self.handle_help(key.code),
                    }
                }
            }
        }

        // 終了前にセッション状態を保存する（端末クリーンアップの成否に関わらず実行）。
        // 保存失敗はベストエフォートとして無視する（終了処理のため通知手段がない）。
        let _ = crate::history::save_session_state(&crate::history::SessionState {
            cursor: self.cursor,
            lines: self.lines.clone(),
            is_daw_mode: self.is_daw_mode,
        });

        let raw_mode_result = disable_raw_mode();
        let alternate_screen_result = execute!(terminal.backend_mut(), LeaveAlternateScreen);
        raw_mode_result?;
        alternate_screen_result?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
