//! DAW モードのキー入力処理

use crossterm::event::KeyCode;

use super::{DawApp, DawMode, DawNormalAction, DawPlayState, MEASURES, TRACKS};

impl DawApp {
    // ─── INSERT モード ────────────────────────────────────────

    pub(super) fn start_insert(&mut self) {
        let mut ta = tui_textarea::TextArea::default();
        for ch in self.data[self.cursor_track][self.cursor_measure].chars() {
            ta.insert_char(ch);
        }
        self.textarea = ta;
        self.mode = DawMode::Insert;
    }

    /// 編集内容を確定してキャッシュ更新・保存を行う
    pub(super) fn commit_insert(&mut self) {
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

        // hot reload: 次の再生ループから新しい MML を反映する
        // ロックを最小限に保つため、build_full_mml() をロック取得前に実行する
        let new_mml = self.build_full_mml();
        *self.play_mml.lock().unwrap() = new_mml;
    }

    // ─── キー処理 ─────────────────────────────────────────────

    pub(super) fn handle_normal(&mut self, key: KeyCode) -> DawNormalAction {
        match key {
            KeyCode::Char('q') => return DawNormalAction::QuitApp,
            KeyCode::Char('d') | KeyCode::Esc => return DawNormalAction::ReturnToTui,

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

                    // hot reload: 次の再生ループから新しい音色を反映する
                    // ロックを最小限に保つため、build_full_mml() をロック取得前に実行する
                    let new_mml = self.build_full_mml();
                    *self.play_mml.lock().unwrap() = new_mml;
                }
            }

            _ => {}
        }
        DawNormalAction::Continue
    }

    pub(super) fn handle_insert(&mut self, key_event: crossterm::event::KeyEvent) {
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
}
