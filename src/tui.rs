//! vim 風 TUI
//!
//! モード:
//!   NORMAL : j/k で行移動、i/o で INSERT、Enter/Space で再生、q で終了
//!   INSERT : tui-textarea で編集
//!            ESC   → 確定 → NORMAL（再生開始）
//!            Enter → 確定 → 次行に新規行挿入 → INSERT 継続

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use tui_textarea::TextArea;

use std::sync::{Arc, Mutex};

use crate::config::Config;

#[derive(PartialEq)]
enum Mode {
    Normal,
    Insert,
}

/// handle_normal の戻り値
enum NormalAction {
    Continue,
    Quit,
    LaunchDaw,
}

#[derive(Clone, PartialEq)]
enum PlayState {
    Idle,
    Running(String),  // レンダリング中
    Playing(String),  // 演奏中
    Done(String),
    Err(String),
}

pub struct TuiApp<'a> {
    mode: Mode,
    lines: Vec<String>,
    cursor: usize,
    list_state: ListState,
    textarea: TextArea<'a>,
    cfg: Arc<Config>,
    entry_ptr: usize, // *const PluginEntry as usize (main() に生存保証)
    play_state: Arc<Mutex<PlayState>>,
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

        let lines = vec!["cde".to_string()];
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            mode: Mode::Normal,
            lines,
            cursor: 0,
            list_state,
            textarea: TextArea::default(),
            cfg: cfg_arc,
            entry_ptr: entry as *const PluginEntry as usize,
            play_state: Arc::new(Mutex::new(PlayState::Idle)),
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

    fn handle_normal(&mut self, key: KeyCode) -> NormalAction {
        match key {
            KeyCode::Char('q') => return NormalAction::Quit,
            KeyCode::Char('d') => return NormalAction::LaunchDaw,
            KeyCode::Char('i') => self.start_insert(),
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

    fn status_text(&self) -> String {
        let play = self.play_state.lock().unwrap().clone();
        let play_str = match play {
            PlayState::Idle           => "".to_string(),
            PlayState::Running(mml)   => format!("  ⚙ レンダリング中: {}", mml),
            PlayState::Playing(msg)   => format!("  ▶ 演奏中: {}", msg),
            PlayState::Done(msg)      => format!("  ✓ {}", msg),
            PlayState::Err(msg)       => format!("  ✗ {}", msg),
        };
        match self.mode {
            Mode::Normal => format!("NORMAL  i:INSERT  j/k:移動  Enter:再生  d:DAW  q:終了{}", play_str),
            Mode::Insert => format!("INSERT  ESC:確定→NORMAL  Enter:確定→次行{}", play_str),
        }
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        loop {
            let status = self.status_text();
            let is_insert = self.mode == Mode::Insert;
            let cursor = self.cursor;
            let status_color = match &*self.play_state.lock().unwrap() {
                PlayState::Err(_)     => Color::Red,
                PlayState::Running(_) => Color::Magenta,
                PlayState::Playing(_) => Color::Yellow,
                PlayState::Done(_)    => Color::Green,
                PlayState::Idle       => Color::Cyan,
            };

            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(3),
                        Constraint::Length(3),
                        Constraint::Length(1),
                    ])
                    .split(f.area());

                let items: Vec<ListItem> = self.lines.iter().enumerate().map(|(i, line)| {
                    let style = if i == cursor {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:>3} ", i + 1), Style::default().fg(Color::DarkGray)),
                        Span::styled(line.clone(), style),
                    ]))
                }).collect();

                f.render_stateful_widget(
                    List::new(items)
                        .block(Block::default().borders(Borders::ALL).title(" MML Lines "))
                        .highlight_symbol("▶ "),
                    chunks[0],
                    &mut self.list_state,
                );

                let insert_block = Block::default()
                    .borders(Borders::ALL)
                    .title(if is_insert { " INSERT " } else { " -- " })
                    .border_style(if is_insert {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    });
                f.render_widget(insert_block, chunks[1]);
                if is_insert {
                    let inner = chunks[1].inner(Margin { horizontal: 1, vertical: 1 });
                    f.render_widget(&self.textarea, inner);
                }

                f.render_widget(
                    Paragraph::new(status.clone()).style(Style::default().fg(status_color)),
                    chunks[2],
                );
            })?;

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
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }
}
