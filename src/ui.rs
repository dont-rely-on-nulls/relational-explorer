use ascii_table::AsciiTable;
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::repl::{InputMode, Repl};

/// Main render entry point
pub fn render(repl: &Repl, frame: &mut Frame) {
    let [help_area, messages_area, input_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .areas(frame.area());

    render_help(repl, frame, help_area);
    render_messages(repl, frame, messages_area);
    render_input(repl, frame, input_area);
}

fn render_help(repl: &Repl, frame: &mut Frame, area: Rect) {
    let widget = help_text(repl.mode)
        .map(|(spans, style)| {
            Paragraph::new(Text::from(Line::from(spans)).patch_style(style))
        })
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
                (" to scroll", false),
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
                (" to stop editing, ", false),
                ("Enter", true),
                (" to submit", false),
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
        Position::new(
            area.x + repl.character_index as u16 + 1,
            area.y + 1,
        )
    })
}

fn render_messages(repl: &Repl, frame: &mut Frame, area: Rect) {
    let widget = messages_widget(repl, area.height);
    frame.render_widget(widget, area);
}

fn messages_widget(repl: &Repl, height: u16) -> Paragraph<'_> {
    let content = build_query_results(&repl.messages);
    let scroll = repl.calculate_scroll_offset(height);

    Paragraph::new(content)
        .block(Block::bordered().title("Query Results"))
        .scroll((scroll, 0))
}

fn build_query_results(messages: &[String]) -> String {
    messages
        .iter()
        .map(|msg| format_query_result(msg))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_query_result(query: &str) -> String {
    format!(
        "sakura=> {}\n\n{}\n(3 rows)\n",
        query,
        mock_table_result()
    )
}

/// Mock database result table (will be replaced with real query execution)
fn mock_table_result() -> String {
    let headers = ["ID", "Name", "Age"];
    let rows = [
        ["1", "Alice", "30"],
        ["2", "Bob", "25"],
        ["3", "Charlie", "35"],
    ];

    render_table(headers, rows)
}

fn render_table<const N: usize>(headers: [&str; N], rows: [[&str; N]; 3]) -> String {
    let mut table = AsciiTable::default();

    headers
        .iter()
        .enumerate()
        .for_each(|(i, &header)| {
            table.column(i).set_header(header);
        });

    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.iter().map(|&s| s.to_string()).collect())
        .collect();

    let mut output = Vec::new();
    table.writeln(&mut output, &data).ok();
    String::from_utf8(output).unwrap_or_default()
}
