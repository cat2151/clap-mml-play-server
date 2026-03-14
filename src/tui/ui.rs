//! TUI 描画

use ratatui::{
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::{Mode, PlayState, TuiApp};

pub(super) fn draw(app: &mut TuiApp<'_>, f: &mut Frame) {
    // play_state を一度だけロックしてスナップショットを取り、
    // status_text と status_color を同じ状態から導出する（二重ロック・状態不整合を防ぐ）。
    let play_state = app.play_state.lock().unwrap().clone();
    let status = status_text(app, &play_state);
    let status_color = status_color(&play_state);

    if app.mode == Mode::PatchSelect {
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
        Mode::Normal => format!("NORMAL  i:INSERT  t:音色選択  j/k:移動  Enter:再生  d:DAW  q:終了{}", play_str),
        Mode::Insert => format!("INSERT  ESC:確定→NORMAL  Enter:確定→次行{}", play_str),
        Mode::PatchSelect => format!("音色選択  Enter:決定  ESC:キャンセル  ↑↓:移動  文字入力:フィルタ  Space:AND条件{}", play_str),
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
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    let items: Vec<ListItem> = app.lines.iter().enumerate().map(|(i, line)| {
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
        &mut app.list_state,
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
        f.render_widget(&app.textarea, inner);
    }

    f.render_widget(
        Paragraph::new(status.to_string()).style(Style::default().fg(status_color)),
        chunks[2],
    );
}
