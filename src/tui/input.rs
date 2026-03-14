//! TUI のキー入力処理

use crossterm::event::KeyCode;
use mmlabc_to_smf::mml_preprocessor;
use ratatui::widgets::ListState;
use tui_textarea::TextArea;

use super::{filter_patches, Mode, NormalAction, PatchLoadState, PlayState, TuiApp};

impl<'a> TuiApp<'a> {
    pub(super) fn start_insert(&mut self) {
        self.textarea = TextArea::default();
        let current = self.lines[self.cursor].clone();
        for ch in current.chars() {
            self.textarea.insert_char(ch);
        }
        self.mode = Mode::Insert;
    }

    pub(super) fn start_patch_select(&mut self) {
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

    pub(super) fn update_patch_filter(&mut self) {
        self.patch_filtered = filter_patches(&self.patch_all, &self.patch_query);
        self.patch_cursor = 0;
        if !self.patch_filtered.is_empty() {
            self.patch_list_state.select(Some(0));
        } else {
            self.patch_list_state.select(None);
        }
    }

    pub(super) fn handle_patch_select(&mut self, key_event: crossterm::event::KeyEvent) {
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

    pub(super) fn handle_normal(&mut self, key: KeyCode) -> NormalAction {
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

    pub(super) fn handle_insert(&mut self, key_event: crossterm::event::KeyEvent) {
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
}
