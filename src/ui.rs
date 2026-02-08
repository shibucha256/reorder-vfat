// reorder-vfat Copyright (c) 2026 shibucha 
// https://github.com/shibucha256/reorder-vfat
// SPDX-License-Identifier: MIT

use crate::app::{App, Mode};
use crate::platform::Platform;
use ratatui::prelude::*;
use ratatui::widgets::*;
use unicode_width::UnicodeWidthStr;

pub(crate) fn ui<P: Platform>(frame: &mut Frame, app: &mut App<P>) {
    let size = frame.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(2)])
        .split(size);

    let title = Paragraph::new(format!(
        "reorder-vfat  |  {}",
        app.current_dir.to_string_lossy()
    ))
    .style(Style::default().fg(Color::Cyan));
    frame.render_widget(title, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(32)])
        .split(chunks[1]);

    match app.mode {
        Mode::SelectDrive => {
            let inner_width = main_chunks[0].width.saturating_sub(2) as usize;
            let items: Vec<ListItem> = app
                .drives
                .iter()
                .map(|d| ListItem::new(pad_to_width(d, inner_width)))
                .collect();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Removable Drives"))
                .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
                .highlight_symbol("> ");
            frame.render_stateful_widget(list, main_chunks[0], &mut app.drive_state);
        }
        _ => {
            let inner_width = main_chunks[0].width.saturating_sub(2) as usize;
            let items: Vec<ListItem> = app
                .entries
                .iter()
                .map(|e| {
                    let prefix = if e.is_dir { "[D] " } else { "    " };
                    let name = e.name.to_string_lossy();
                    let line = format!("{prefix}{name}");
                    ListItem::new(pad_to_width(&line, inner_width))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Entries"))
                .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
                .highlight_symbol("> ");

            frame.render_stateful_widget(list, main_chunks[0], &mut app.list_state);
        }
    }

    let help = Paragraph::new(help_lines(app.mode))
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .wrap(Wrap { trim: true });
    frame.render_widget(help, main_chunks[1]);

    match app.mode {
        Mode::Renaming => {
            let area = centered_rect(60, 20, size);
            frame.render_widget(Clear, area);
            let block = Block::default().borders(Borders::ALL).title("Rename");
            let label = "New name: ";
            let inner_width = area.width.saturating_sub(2) as usize;
            let max_width = inner_width.saturating_sub(label.width());
            let (visible, cursor_offset) =
                visible_with_cursor(&app.rename_input, app.rename_cursor, max_width);
            let text = Paragraph::new(format!("{label}{visible}"))
                .block(block)
                .wrap(Wrap { trim: true });
            frame.render_widget(text, area);
            let cursor_x = area.x + 1 + label.width() as u16 + cursor_offset as u16;
            let cursor_y = area.y + 1;
            frame.set_cursor(cursor_x.min(area.x + area.width - 2), cursor_y);
        }
        Mode::ConfirmSort => {
            let area = centered_rect(60, 20, size);
            frame.render_widget(Clear, area);
            let block = Block::default().borders(Borders::ALL).title("Confirm");
            let text = Paragraph::new("Write VFAT order? (y/n)")
                .block(block)
                .wrap(Wrap { trim: true });
            frame.render_widget(text, area);
        }
        _ => {}
    }

    let status = match app.mode {
        Mode::Normal => {
            let msg = app.message.clone().unwrap_or_default();
            format!("竊鯛・ move  Enter open  Backspace up  r rename  L drives  w write VFAT order  q quit  {msg}")
        }
        Mode::Renaming => format!("Rename to: {}", app.rename_input),
        Mode::ConfirmSort => "Write VFAT order? y/n".to_string(),
        Mode::SelectDrive => {
            if let Some(msg) = &app.message {
                msg.clone()
            } else {
                "Select drive: Enter open  F5 reload  Esc cancel".to_string()
            }
        }
    };
    let status_bar = Paragraph::new(status).block(Block::default().borders(Borders::ALL));
    frame.render_widget(status_bar, chunks[2]);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn visible_with_cursor(input: &str, cursor: usize, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }
    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());

    let mut start = 0usize;
    while start < cursor {
        let width_start: usize = chars[start..cursor]
            .iter()
            .map(|c| c.to_string().width())
            .sum();
        if width_start <= max_width.saturating_sub(1) {
            break;
        }
        start += 1;
    }

    let mut end = start;
    let mut width = 0usize;
    while end < chars.len() {
        let w = chars[end].to_string().width();
        if width + w > max_width {
            break;
        }
        width += w;
        end += 1;
    }

    let visible: String = chars[start..end].iter().collect();
    let cursor_offset: usize = chars[start..cursor]
        .iter()
        .map(|c| c.to_string().width())
        .sum();
    (visible, cursor_offset)
}

fn pad_to_width(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        return s.to_string();
    }
    let mut out = String::with_capacity(width);
    out.push_str(s);
    out.push_str(&" ".repeat(width - w));
    out
}

fn help_lines(mode: Mode) -> Vec<Line<'static>> {
    match mode {
        Mode::Normal => vec![
            Line::from("↑/↓  move"),
            Line::from("Enter  open"),
            Line::from("Backspace  up"),
            Line::from("Ins  move down"),
            Line::from("Del  move up"),
            Line::from("R  rename"),
            Line::from("S  sort"),
            Line::from("L  drives"),
            Line::from("W  write VFAT order"),
            Line::from("Q  quit"),
        ],
        Mode::Renaming => vec![
            Line::from("←/→  move cursor"),
            Line::from("Home/End"),
            Line::from("Backspace  delete"),
            Line::from("Enter  apply"),
            Line::from("Esc  cancel"),
        ],
        Mode::ConfirmSort => vec![Line::from("y  write"), Line::from("n  cancel")],
        Mode::SelectDrive => vec![
            Line::from("↑/↓  move"),
            Line::from("Enter  open"),
            Line::from("F5  reload"),
            Line::from("Esc  cancel"),
        ],
    }
}
