//! vim 風 TUI
//!
//! モード:
//!   NORMAL : j/k で行移動、i/o で INSERT、t で音色選択、Enter/Space で再生、q で終了
//!   INSERT : tui-textarea で編集
//!            ESC   → 確定 → NORMAL（再生開始）
//!            Enter → 確定 → 次行に新規行挿入 → INSERT 継続
//!   PATCHSELECT : インクリメンタルサーチで音色を選択
//!            文字入力: フィルタ（space=AND条件）
//!            ↑↓:リスト移動  Enter:現在行の先頭にJSONで挿入（上書き）  ESC:キャンセル

mod ui;

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use mmlabc_to_smf::mml_preprocessor;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::ListState, Frame, Terminal};
use tui_textarea::TextArea;

use std::sync::{Arc, Mutex};

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
    // 音色選択モード用
    /// バックグラウンドスレッドが収集したパッチリストの状態
    patch_load_state: Arc<Mutex<PatchLoadState>>,
    /// PatchSelect 起動時にスナップショットした (表示名, 小文字化済み) ペアのリスト
    pub(super) patch_all: Vec<(String, String)>,
    pub(super) patch_query: String,          // 検索クエリ
    pub(super) patch_filtered: Vec<String>,  // フィルタ結果（表示名のみ）
    pub(super) patch_cursor: usize,          // フィルタ結果内のカーソル位置
    pub(super) patch_list_state: ListState,  // 音色選択リスト描画用
}

impl<'a> TuiApp<'a> {
    pub fn new(cfg: &'a Config, entry: &'a PluginEntry) -> Self {
        let cfg_arc = Arc::new(Config {
            plugin_path: cfg.plugin_path.clone(),
            input_midi:  cfg.input_midi.clone(),
            output_midi: cfg.output_midi.clone(),
            output_wav:  cfg.output_wav.clone(),
            sample_rate: cfg.sample_rate,
            buffer_size: cfg.buffer_size,
            patch_path: cfg.patch_path.clone(),
            patches_dir: cfg.patches_dir.clone(),
            random_patch: cfg.random_patch,
        });

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
        let lines = vec!["cde".to_string()];
        let session = crate::history::load_session_state();
        let initial_cursor = session.cursor.min(lines.len() - 1);
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
            patch_load_state,
            patch_all: Vec::new(),
            patch_query: String::new(),
            patch_filtered: Vec::new(),
            patch_cursor: 0,
            patch_list_state: ListState::default(),
        }
    }

    fn kick_play(&self, mml: String) {
        let cfg = Arc::clone(&self.cfg);
        let state = Arc::clone(&self.play_state);
        let entry_ptr = self.entry_ptr;

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
                    let msg = format!("{} | {}", patch_name, mml);
                    // 演奏中に切り替え
                    *state.lock().unwrap() = PlayState::Playing(msg.clone());

                    // 再生（ブロッキング）
                    let play_result = crate::pipeline::play_samples(samples, cfg.sample_rate as u32);

                    *state.lock().unwrap() = match play_result {
                        Ok(_)  => PlayState::Done(format!("✓ {}", msg)),
                        Err(e) => PlayState::Err(format!("エラー: {}", e)),
                    };
                }
            }
        });
    }

    fn start_insert(&mut self) {
        self.textarea = TextArea::default();
        let current = self.lines[self.cursor].clone();
        for ch in current.chars() {
            self.textarea.insert_char(ch);
        }
        self.mode = Mode::Insert;
    }

    fn start_patch_select(&mut self) {
        // ロードが完了したパッチリストをスナップショットする
        {
            let state = self.patch_load_state.lock().unwrap();
            if let PatchLoadState::Ready(pairs) = &*state {
                self.patch_all = pairs.clone();
            }
        }
        self.patch_query = String::new();
        self.patch_filtered = self.patch_all.iter().map(|(orig, _)| orig.clone()).collect();
        self.patch_cursor = 0;
        let mut ls = ListState::default();
        if !self.patch_filtered.is_empty() {
            ls.select(Some(0));
        }
        self.patch_list_state = ls;
        self.mode = Mode::PatchSelect;
    }

    fn update_patch_filter(&mut self) {
        self.patch_filtered = filter_patches(&self.patch_all, &self.patch_query);
        self.patch_cursor = 0;
        if !self.patch_filtered.is_empty() {
            self.patch_list_state.select(Some(0));
        } else {
            self.patch_list_state.select(None);
        }
    }

    fn handle_patch_select(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                if !self.patch_filtered.is_empty() {
                    let selected = self.patch_filtered[self.patch_cursor].clone();
                    // serde_json を使って値を適切にエスケープする（パスに引用符・バックスラッシュが含まれる場合も安全）
                    let json = format!(
                        "{{\"Surge XT patch\": {}}}",
                        serde_json::to_string(&selected).unwrap_or_else(|_| format!("\"{}\"", selected))
                    );
                    // 現在行の既存JSON（あれば）を除去して先頭に新しいJSONを挿入する
                    let current = self.lines[self.cursor].clone();
                    let preprocessed = mml_preprocessor::extract_embedded_json(&current);
                    let remaining = preprocessed.remaining_mml.trim().to_string();
                    self.lines[self.cursor] = if remaining.is_empty() {
                        json
                    } else {
                        format!("{} {}", json, remaining)
                    };
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Down => {
                if self.patch_cursor + 1 < self.patch_filtered.len() {
                    self.patch_cursor += 1;
                    self.patch_list_state.select(Some(self.patch_cursor));
                }
            }
            KeyCode::Up => {
                if self.patch_cursor > 0 {
                    self.patch_cursor -= 1;
                    self.patch_list_state.select(Some(self.patch_cursor));
                }
            }
            KeyCode::Backspace => {
                self.patch_query.pop();
                self.update_patch_filter();
            }
            KeyCode::Char(c) => {
                self.patch_query.push(c);
                self.update_patch_filter();
            }
            _ => {}
        }
    }

    fn handle_normal(&mut self, key: KeyCode) -> NormalAction {
        match key {
            KeyCode::Char('q') => return NormalAction::Quit,
            KeyCode::Char('d') => return NormalAction::LaunchDaw,
            KeyCode::Char('i') => self.start_insert(),
            KeyCode::Char('t') => {
                if self.cfg.random_patch {
                    *self.play_state.lock().unwrap() = PlayState::Err(
                        "random音色モードでは音色選択は使えません".to_string(),
                    );
                } else if self.cfg.patches_dir.is_none() {
                    *self.play_state.lock().unwrap() = PlayState::Err(
                        "patches_dir が設定されていません".to_string(),
                    );
                } else {
                    // バックグラウンドロードの状態を確認する
                    let action = {
                        let state = self.patch_load_state.lock().unwrap();
                        match &*state {
                            PatchLoadState::Loading => Err("パッチを読み込み中です...".to_string()),
                            PatchLoadState::Err(e)  => Err(format!("パッチの読み込みに失敗: {}", e)),
                            PatchLoadState::Ready(p) if p.is_empty() => {
                                Err("patches_dir にパッチが見つかりません".to_string())
                            }
                            PatchLoadState::Ready(_) => Ok(()),
                        }
                    };
                    match action {
                        Err(msg) => *self.play_state.lock().unwrap() = PlayState::Err(msg),
                        Ok(())   => self.start_patch_select(),
                    }
                }
            }
            KeyCode::Char('o') => {
                self.lines.insert(self.cursor + 1, String::new());
                self.cursor += 1;
                self.list_state.select(Some(self.cursor));
                self.start_insert();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.cursor + 1 < self.lines.len() {
                    self.cursor += 1;
                    self.list_state.select(Some(self.cursor));
                }
            }
            KeyCode::Char('k') => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.list_state.select(Some(self.cursor));
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let mml = self.lines[self.cursor].trim().to_string();
                if !mml.is_empty() {
                    self.kick_play(mml);
                }
            }
            _ => {}
        }
        NormalAction::Continue
    }

    fn handle_insert(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            KeyCode::Esc => {
                let text = self.textarea.lines().join("");
                self.lines[self.cursor] = text.clone();
                self.mode = Mode::Normal;
                if !text.trim().is_empty() {
                    self.kick_play(text.trim().to_string());
                }
            }
            KeyCode::Enter => {
                // 確定 → 非同期再生 → 次行挿入 → INSERT 継続
                let text = self.textarea.lines().join("");
                self.lines[self.cursor] = text.clone();
                if !text.trim().is_empty() {
                    self.kick_play(text.trim().to_string());
                }
                self.lines.insert(self.cursor + 1, String::new());
                self.cursor += 1;
                self.list_state.select(Some(self.cursor));
                self.textarea = TextArea::default();
            }
            _ => {
                self.textarea.input(key_event);
            }
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

        loop {
            terminal.draw(|f| self.draw(f))?;

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
                                    daw.run_with_terminal(&mut terminal)?;
                                }
                                NormalAction::Continue => {}
                            }
                        }
                        Mode::Insert => self.handle_insert(key),
                        Mode::PatchSelect => self.handle_patch_select(key),
                    }
                }
            }
        }

        // 終了前にセッション状態を保存する（端末クリーンアップの成否に関わらず実行）。
        // 保存失敗はベストエフォートとして無視する（終了処理のため通知手段がない）。
        let _ = crate::history::save_session_state(&crate::history::SessionState {
            cursor: self.cursor,
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
