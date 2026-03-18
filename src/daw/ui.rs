//! DAW モードの描画

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::{CacheState, DawApp, DawMode, DawPlayState};

/// Pending インジケータのアニメーション 1 フレームの長さ（ミリ秒）
const ANIM_FRAME_MS: u128 = 250;
/// Pending インジケータのアニメーションフレーム数（"." / ".." / "..."）
const ANIM_FRAME_COUNT: u128 = 3;

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

    if app.mode == DawMode::Help {
        draw_help(f, area);
    }
}

fn draw_grid(app: &DawApp, f: &mut Frame, area: Rect) {
    // キャッシュ状態をスナップショットしてからロックを解放する。
    // これによりキャッシュワーカースレッドとの競合を最小化する。
    let cache_states: Vec<Vec<CacheState>> = {
        let cache = app.cache.lock().unwrap();
        (0..app.tracks)
            .map(|t| (0..=app.measures).map(|m| cache[t][m].state.clone()).collect())
            .collect()
    };

    // ヘッダ行（列ラベル）
    let mut header_spans = vec![Span::styled("     ", Style::default())];
    for m in 0..=app.measures {
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

    // Pending セル用アニメーションフレーム（0..ANIM_FRAME_COUNT を ANIM_FRAME_MS ごとに切り替え）
    let anim_frame = {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        (millis / ANIM_FRAME_MS) % ANIM_FRAME_COUNT
    };

    // track 行（2 行ずつ）
    for t in 0..app.tracks {
        let row_y = area.y + 1 + (t as u16) * 2;
        if row_y + 1 >= area.y + area.height {
            break;
        }

        let is_cursor_track = t == app.cursor_track;

        // 行 1: track ラベル + セル内容 (4 chars each)
        let mut row1: Vec<Span> = vec![Span::styled(
            format!("T{:<2}  ", t),
            if is_cursor_track {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
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

        for m in 0..=app.measures {
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
                (Color::Black, Color::Cyan)
            } else {
                match cs {
                    CacheState::Empty => (Color::DarkGray, Color::Reset),
                    CacheState::Pending => (Color::DarkGray, Color::Reset),
                    CacheState::Ready => (Color::White, Color::Reset),
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
                    CacheState::Pending => match anim_frame {
                        0 => ".    ",
                        1 => "..   ",
                        _ => "...  ",
                    },
                    CacheState::Ready => "     ",
                    CacheState::Error => "✗    ",
                };
                let ind_fg = if is_cursor {
                    Color::Cyan
                } else {
                    match cs {
                        CacheState::Empty => Color::DarkGray,
                        CacheState::Pending => Color::DarkGray,
                        CacheState::Ready => Color::DarkGray,
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
    // play_state と play_position を一度だけロックしてスナップショットを取る。
    let play_state = app.play_state.lock().unwrap().clone();
    let play_position = app.play_position.lock().unwrap().clone();

    // 拍子・テンポは常に現在の app 状態から取得することで、
    // hot reload 後もビート表示が正確に保たれる。
    let beat_count = app.beat_numerator();
    let beat_duration_secs = 60.0 / app.tempo_bpm();

    let play_str = match &play_state {
        DawPlayState::Idle => "".to_string(),
        DawPlayState::Playing | DawPlayState::Preview => {
            let label = if play_state == DawPlayState::Preview { "PREVIEW" } else { "loop" };
            let pos_str = if let Some(pos) = &play_position {
                let elapsed = pos.measure_start.elapsed().as_secs_f64();
                let raw_beat = (elapsed / beat_duration_secs) as u32;
                let current_beat = (raw_beat % beat_count) + 1;
                format!(
                    "  ▶ meas{}, beat{} ({})",
                    pos.measure_index + 1,
                    current_beat,
                    label
                )
            } else {
                format!("  ▶ 演奏中 ({})", label)
            };
            pos_str
        }
    };

    let text = match app.mode {
        DawMode::Normal => format!(
            "DAW  h/l:小節移動  j/k:track移動  i:INSERT  p:play/stop  r:random音色  K:ヘルプ  d/ESC:戻る  q:終了{}",
            play_str
        ),
        DawMode::Insert => format!(
            "ESC:確定→NORMAL  Enter:確定→次小節{}",
            play_str
        ),
        DawMode::Help => format!(
            "HELP  ESC:キャンセル{}",
            play_str
        ),
    };

    let color = match play_state {
        DawPlayState::Idle => Color::Cyan,
        DawPlayState::Playing => Color::Yellow,
        DawPlayState::Preview => Color::Magenta,
    };

    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(color)),
        area,
    );
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup = crate::ui_utils::centered_rect(60, 80, area);
    f.render_widget(Clear, popup);

    let help_lines = vec![
        Line::from(Span::styled("NORMAL モード", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  h / ←  : 小節移動（左）"),
        Line::from("  l / →  : 小節移動（右）"),
        Line::from("  j / ↓  : track 移動（下）"),
        Line::from("  k / ↑  : track 移動（上）"),
        Line::from("  H      : 先頭 track へ移動"),
        Line::from("  M      : 中央 track へ移動"),
        Line::from("  L      : 末尾 track へ移動"),
        Line::from("  i      : INSERT モード"),
        Line::from("  p      : 演奏 / 停止"),
        Line::from("  r      : random 音色設定"),
        Line::from("  K      : ヘルプ (このページ)"),
        Line::from("  d/ESC  : TUI に戻る"),
        Line::from("  q      : 終了"),
        Line::from("  Ctrl+C : 強制終了"),
        Line::from(""),
        Line::from(Span::styled("INSERT モード", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  ESC   : 確定 → NORMAL"),
        Line::from("  Enter : 確定 → 次小節 → INSERT 継続"),
        Line::from("  ;     : 分割して下の track に追加"),
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
        popup,
    );
}
