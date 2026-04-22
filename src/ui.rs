use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Clear, Paragraph},
    Frame,
};

use crate::repl::{InputMode, Repl};

/// Main render entry point
pub fn render(repl: &Repl, frame: &mut Frame) {
    // Calculate input area height based on number of lines (min 3, max 15)
    let input_lines = repl.input.lines().count().max(1) + 2; // +2 for borders
    let input_height = (input_lines as u16).min(15).max(3);

    let [help_area, messages_area, input_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(input_height),
    ])
    .areas(frame.area());

    render_help(repl, frame, help_area);
    render_messages(repl, frame, messages_area);
    render_input(repl, frame, input_area);

    if let Some((kind, msg)) = &repl.error_popup {
        render_error_popup(frame, kind, msg);
    }
}

fn render_help(repl: &Repl, frame: &mut Frame, area: Rect) {
    let widget = help_text(repl.mode)
        .map(|(spans, style)| Paragraph::new(Text::from(Line::from(spans)).patch_style(style)))
        .unwrap_or_default();

    frame.render_widget(widget, area);
}

fn help_text(mode: InputMode) -> Option<(Vec<Span<'static>>, Style)> {
    use InputMode::*;
    match mode {
        Normal => Some((
            [
                ("Press ", false),
                ("q", true),
                (" to exit, ", false),
                ("e", true),
                (" to edit, ", false),
                ("↑↓", true),
                (" to scroll, ", false),
                ("y", true),
                (" to copy last result", false),
            ]
            .into_iter()
            .map(|(text, bold)| if bold { text.bold() } else { text.into() })
            .collect(),
            Style::default().add_modifier(Modifier::RAPID_BLINK),
        )),
        Editing => Some((
            [
                ("Press ", false),
                ("Esc", true),
                (" to stop, ", false),
                ("Alt+Enter", true),
                (" to submit, ", false),
                ("Enter", true),
                (" for newline, ", false),
                ("↑↓", true),
                (" history, ", false),
                ("Ctrl+Y", true),
                (" to copy", false),
            ]
            .into_iter()
            .map(|(text, bold)| if bold { text.bold() } else { text.into() })
            .collect(),
            Style::default(),
        )),
    }
}

fn render_input(repl: &Repl, frame: &mut Frame, area: Rect) {
    let widget = input_widget(repl);
    frame.render_widget(widget, area);

    cursor_position(repl, area)
        .into_iter()
        .for_each(|pos| frame.set_cursor_position(pos));
}

fn input_widget(repl: &Repl) -> Paragraph<'_> {
    let style = input_style(repl.mode);
    Paragraph::new(repl.input.as_str())
        .style(style)
        .block(Block::bordered().title("Input"))
}

fn input_style(mode: InputMode) -> Style {
    match mode {
        InputMode::Normal => Style::default(),
        InputMode::Editing => Style::default().fg(Color::Yellow),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn cursor_position(repl: &Repl, area: Rect) -> Option<Position> {
    matches!(repl.mode, InputMode::Editing).then(|| {
        // Count characters up to cursor position and find which line it's on
        let mut cursor_row = 0;
        let mut line_start = 0;

        for (i, ch) in repl.input.chars().enumerate() {
            if i == repl.character_index {
                break;
            }
            if ch == '\n' {
                cursor_row += 1;
                line_start = i + 1;
            }
        }

        let line_offset = repl.character_index - line_start;

        // Clamp cursor row to stay inside the border (last usable row = area.height - 2)
        let max_row = area.height.saturating_sub(2) as usize;
        let visible_row = cursor_row.min(max_row);

        Position::new(
            area.x + line_offset as u16 + 1,
            area.y + visible_row as u16 + 1,
        )
    })
}

fn render_messages(repl: &Repl, frame: &mut Frame, area: Rect) {
    let widget = messages_widget(repl, area.height);
    frame.render_widget(widget, area);
}

fn messages_widget(repl: &Repl, height: u16) -> Paragraph<'_> {
    let content = build_query_results(repl);
    let scroll = repl.calculate_scroll_offset(height);
    let title = format!(
        "Query Results  [db: {} | branch: {} | hash: {}]",
        repl.db_name, repl.branch, repl.db_hash
    );

    Paragraph::new(content)
        .block(Block::bordered().title(title))
        .scroll((scroll, 0))
}

fn render_error_popup(frame: &mut Frame, kind: &str, msg: &str) {
    let area = frame.area();
    let width = (area.width * 2 / 3)
        .max(44)
        .min(area.width.saturating_sub(4));

    let inner_w = width.saturating_sub(2) as usize; // 2 for borders
    let body_lines: u16 = msg
        .lines()
        .map(|l| ((l.len().max(1) + inner_w - 1) / inner_w) as u16)
        .sum::<u16>()
        .max(1);
    // borders(2) + body + footer(1) + padding(1)
    let height = (body_lines + 4).min(area.height.saturating_sub(4));

    let popup_area = Rect::new(
        (area.width.saturating_sub(width)) / 2,
        (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );

    frame.render_widget(Clear, popup_area);

    let block = Block::bordered()
        .title(format!(" {} ", kind))
        .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Split inner area: body | footer hint
    let [body_area, footer_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let body = Paragraph::new(msg).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(body, body_area);

    let hint = "[ press any key to close ]";
    let hint_x = footer_area.x + footer_area.width.saturating_sub(hint.len() as u16) / 2;
    let close_btn = Paragraph::new(hint).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(
        close_btn,
        Rect::new(hint_x, footer_area.y, hint.len() as u16, 1),
    );
}

fn build_query_results(repl: &Repl) -> String {
    crate::repl::build_query_results(repl)
}
