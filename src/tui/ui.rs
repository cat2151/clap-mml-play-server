//! TUI 描画

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use super::{Mode, PlayState, TuiApp};

pub(super) fn draw(app: &mut TuiApp<'_>, f: &mut Frame) {
    // play_state を一度だけロックしてスナップショットを取り、
    // status_text と status_color を同じ状態から導出する（二重ロック・状態不整合を防ぐ）。
    let play_state = app.play_state.lock().unwrap().clone();
    let status = status_text(app, &play_state);
    let status_color = status_color(&play_state);

    if app.mode == Mode::Help {
        draw_normal(app, f, &status, status_color);
        draw_help(f);
    } else if app.mode == Mode::PatchSelect {
        draw_patch_select(app, f, &status, status_color);
    } else {
        draw_normal(app, f, &status, status_color);
    }
}

fn status_color(play_state: &PlayState) -> Color {
    match play_state {
        PlayState::Err(_)     => Color::Red,
        PlayState::Running(_) => Color::Magenta,
        PlayState::Playing(_) => Color::Yellow,
        PlayState::Done(_)    => Color::Green,
        PlayState::Idle       => Color::Cyan,
    }
}

fn status_text(app: &TuiApp<'_>, play_state: &PlayState) -> String {
    let play_str = match play_state {
        PlayState::Idle           => "".to_string(),
        PlayState::Running(mml)   => format!("  ⚙ レンダリング中: {}", mml),
        PlayState::Playing(msg)   => format!("  ▶ 演奏中: {}", msg),
        PlayState::Done(msg)      => format!("  ✓ {}", msg),
        PlayState::Err(msg)       => format!("  ✗ {}", msg),
    };
    match app.mode {
        Mode::Normal => format!("NORMAL  i:INSERT  t:音色選択  j/k:移動  Enter:再生  d:DAW  K:ヘルプ  q:終了{}", play_str),
        Mode::Insert => format!("ESC:確定→NORMAL  Enter:確定→次行{}", play_str),
        Mode::PatchSelect => format!("音色選択  Enter:決定  ESC:キャンセル  ↑↓:移動  文字入力:フィルタ  Space:AND条件{}", play_str),
        Mode::Help => format!("HELP  ESC:キャンセル{}", play_str),
    }
}

fn draw_patch_select(app: &mut TuiApp<'_>, f: &mut Frame, status: &str, status_color: Color) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    f.render_widget(
        Paragraph::new(format!("> {}", app.patch_query))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 音色選択 - 検索 (space=AND) ")
                    .border_style(Style::default().fg(Color::Yellow)),
            ),
        chunks[0],
    );

    let count_title = format!(
        " パッチ ({}/{}) ",
        app.patch_filtered.len(),
        app.patch_all.len()
    );
    let patch_items: Vec<ListItem> = app
        .patch_filtered
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let style = if i == app.patch_cursor {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Span::styled(p.clone(), style))
        })
        .collect();

    f.render_stateful_widget(
        List::new(patch_items)
            .block(Block::default().borders(Borders::ALL).title(count_title))
            .highlight_symbol("▶ "),
        chunks[1],
        &mut app.patch_list_state,
    );

    f.render_widget(
        Paragraph::new(status.to_string()).style(Style::default().fg(status_color)),
        chunks[2],
    );
}

fn draw_normal(app: &mut TuiApp<'_>, f: &mut Frame, status: &str, status_color: Color) {
    let is_insert = app.mode == Mode::Insert;
    let cursor = app.cursor;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    // キャッシュのガードを保持したままイテレートすることで、全キーのクローンを避ける。
    let cache_guard = app.audio_cache.lock().unwrap();
    let items: Vec<ListItem> = app.lines.iter().enumerate().map(|(i, line)| {
        let mml = line.trim();
        let is_cached = !mml.is_empty() && cache_guard.contains_key(mml);
        let style = if i == cursor {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_cached {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{:>3} ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::styled(line.clone(), style),
        ]))
    }).collect();
    drop(cache_guard);

    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" MML Lines "))
            .highlight_symbol("▶ "),
        chunks[0],
        &mut app.list_state,
    );

    // INSERTモード時は、カーソル行にインラインで textarea を描画する。
    // List ウィジェットは Borders::ALL を持つため、内側の開始は +1 ずつオフセットする。
    if is_insert {
        let list_area = chunks[0];
        let offset = app.list_state.offset();
        if cursor >= offset {
            let row_in_visible = (cursor - offset) as u16;
            let inner_top = list_area.y + 1; // 上ボーダーの内側（1行分）
            let inner_bottom = list_area.y + list_area.height.saturating_sub(1); // 下ボーダーの位置
            let textarea_y = inner_top + row_in_visible;
            if textarea_y < inner_bottom {
                let textarea_area = Rect {
                    x: list_area.x + 1,
                    y: textarea_y,
                    width: list_area.width.saturating_sub(2),
                    height: 1,
                };
                f.render_widget(&app.textarea, textarea_area);
            }
        }
    }

    f.render_widget(
        Paragraph::new(status.to_string()).style(Style::default().fg(status_color)),
        chunks[1],
    );
}

fn draw_help(f: &mut Frame) {
    let area = crate::ui_utils::centered_rect(60, 80, f.area());
    f.render_widget(Clear, area);

    let help_lines = vec![
        Line::from(Span::styled("NORMAL モード", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  j / ↓       : 下へ移動"),
        Line::from("  k / ↑       : 上へ移動"),
        Line::from("  H           : 先頭行へ移動"),
        Line::from("  M           : 中央行へ移動"),
        Line::from("  L           : 末尾行へ移動"),
        Line::from("  Enter/Space : 再生"),
        Line::from("  i           : INSERT モード"),
        Line::from("  o           : 次行に挿入 → INSERT"),
        Line::from("  t           : 音色選択"),
        Line::from("  d           : DAW モード"),
        Line::from("  K           : ヘルプ (このページ)"),
        Line::from("  q           : 終了"),
        Line::from("  Ctrl+C      : 強制終了"),
        Line::from(""),
        Line::from(Span::styled("INSERT モード", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  ESC   : 確定 → NORMAL (再生)"),
        Line::from("  Enter : 確定 → 次行挿入 → INSERT 継続"),
        Line::from(""),
        Line::from(Span::styled("音色選択モード", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  文字入力 : フィルタ (Space=AND条件)"),
        Line::from("  ↑↓      : リスト移動"),
        Line::from("  Enter   : 音色決定"),
        Line::from("  ESC     : キャンセル"),
        Line::from(""),
        Line::from(Span::styled("  [ESC] でキャンセル", Style::default().fg(Color::DarkGray))),
    ];

    f.render_widget(
        Paragraph::new(help_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" ヘルプ (Keybinds) ")
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
        area,
    );
}
