//! DAW モードの描画

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::{CacheState, DawApp, DawMode, DawPlayState, MEASURES, TRACKS};

pub(super) fn draw(app: &DawApp, f: &mut Frame) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    draw_grid(app, f, chunks[0]);
    draw_status(app, f, chunks[1]);
}

fn draw_grid(app: &DawApp, f: &mut Frame, area: Rect) {
    // キャッシュ状態をスナップショットしてからロックを解放する。
    // これによりキャッシュワーカースレッドとの競合を最小化する。
    let cache_states: Vec<Vec<CacheState>> = {
        let cache = app.cache.lock().unwrap();
        (0..TRACKS)
            .map(|t| (0..=MEASURES).map(|m| cache[t][m].state.clone()).collect())
            .collect()
    };

    // ヘッダ行（列ラベル）
    let mut header_spans = vec![Span::styled("     ", Style::default())];
    for m in 0..=MEASURES {
        let label = if m == 0 {
            " Tmb".to_string()
        } else {
            format!(" M{:<2}", m)
        };
        header_spans.push(Span::styled(
            format!("{:<5}", label),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if area.height > 0 {
        f.render_widget(
            Paragraph::new(Line::from(header_spans)),
            Rect { x: area.x, y: area.y, width: area.width, height: 1 },
        );
    }

    // track 行（2 行ずつ）
    for t in 0..TRACKS {
        let row_y = area.y + 1 + (t as u16) * 2;
        if row_y + 1 >= area.y + area.height {
            break;
        }

        let is_cursor_track = t == app.cursor_track;

        // 行 1: track ラベル + セル内容 (4 chars each)
        let mut row1: Vec<Span> = vec![Span::styled(
            format!("T{:<2}  ", t),
            if is_cursor_track {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )];

        // INSERTモード時はカーソルtrackのインジケータ行（行2）が不要なので生成をスキップする。
        let show_indicators = !(is_cursor_track && app.mode == DawMode::Insert);
        let mut row2: Vec<Span> = if show_indicators {
            vec![Span::styled("     ", Style::default())]
        } else {
            vec![]
        };

        for m in 0..=MEASURES {
            let is_cursor = is_cursor_track && m == app.cursor_measure;
            let mml = &app.data[t][m];
            let cs = &cache_states[t][m];

            // セル表示 (4 chars)
            let display: String = if mml.is_empty() {
                "    ".to_string()
            } else {
                let s: String = mml.chars().take(4).collect();
                format!("{:<4}", s)
            };

            let (fg, bg) = if is_cursor {
                (Color::Black, Color::Yellow)
            } else {
                match cs {
                    CacheState::Empty => (Color::DarkGray, Color::Reset),
                    CacheState::Pending => (Color::White, Color::Reset),
                    CacheState::Ready => (Color::Green, Color::Reset),
                    CacheState::Error => (Color::Red, Color::Reset),
                }
            };

            row1.push(Span::styled(
                format!("{} ", display),
                Style::default().fg(fg).bg(bg),
            ));

            // 状態インジケータ (4 chars + 1 space): INSERTモードのカーソルtrackはスキップ
            if show_indicators {
                let indicator = match cs {
                    CacheState::Empty => "     ",
                    CacheState::Pending => "...  ",
                    CacheState::Ready => "●    ",
                    CacheState::Error => "✗    ",
                };
                let ind_fg = if is_cursor {
                    Color::Yellow
                } else {
                    match cs {
                        CacheState::Empty => Color::DarkGray,
                        CacheState::Pending => Color::Yellow,
                        CacheState::Ready => Color::Green,
                        CacheState::Error => Color::Red,
                    }
                };
                row2.push(Span::styled(indicator, Style::default().fg(ind_fg)));
            }
        }

        f.render_widget(
            Paragraph::new(Line::from(row1)),
            Rect { x: area.x, y: row_y, width: area.width, height: 1 },
        );

        // INSERTモード時は、カーソルtrackのインジケータ行にインラインで textarea を描画する。
        if show_indicators {
            f.render_widget(
                Paragraph::new(Line::from(row2)),
                Rect { x: area.x, y: row_y + 1, width: area.width, height: 1 },
            );
        } else {
            f.render_widget(
                &app.textarea,
                Rect { x: area.x, y: row_y + 1, width: area.width, height: 1 },
            );
        }
    }
}

fn draw_status(app: &DawApp, f: &mut Frame, area: Rect) {
    // play_state を一度だけロックしてスナップショットを取り、
    // play_str と color を同じ状態から導出する（二重ロック・状態不整合を防ぐ）。
    let play_state = app.play_state.lock().unwrap().clone();

    let play_str = match play_state {
        DawPlayState::Idle => "".to_string(),
        DawPlayState::Playing => "  ▶ 演奏中 (loop)".to_string(),
    };

    let text = match app.mode {
        DawMode::Normal => format!(
            "DAW  h/l:小節移動  j/k:track移動  i:INSERT  p:play/stop  r:random音色  q:戻る{}",
            play_str
        ),
        DawMode::Insert => format!(
            "ESC:確定→NORMAL  Enter:確定→次小節{}",
            play_str
        ),
    };

    let color = match play_state {
        DawPlayState::Idle => Color::Cyan,
        DawPlayState::Playing => Color::Yellow,
    };

    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(color)),
        area,
    );
}
