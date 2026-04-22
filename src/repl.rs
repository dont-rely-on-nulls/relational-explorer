use color_eyre::Result;
use ratatui::DefaultTerminal;
use std::fmt::Write as _;

use crate::connection::{self, format_response, Connection, ServerResponse};

pub struct QueryEntry {
    pub input: String,
    pub rendered: String,
}

/// REPL session state
pub struct Repl {
    pub input: String,
    pub character_index: usize,
    pub mode: InputMode,
    pub messages: Vec<QueryEntry>,
    /// Offset from auto-scroll position (negative = scrolled up into history)
    pub manual_scroll: i32,
    pub connection: Option<Connection>,
    /// Server address used for (re)connection — TCP `host:port` or Unix socket path.
    pub server_addr: String,
    pub db_hash: String,
    pub db_name: String,
    pub branch: String,
    /// Index into messages for history recall (None = current input)
    pub history_index: Option<usize>,
    /// Current line number in multiline input
    pub cursor_line: usize,
    /// Active error popup (kind, message), if any
    pub error_popup: Option<(String, String)>,
}

#[derive(Clone, Copy)]
pub enum InputMode {
    Normal,
    Editing,
}

impl Repl {
    pub fn new(connection: Option<Connection>, server_addr: String) -> Self {
        Self {
            input: String::new(),
            mode: InputMode::Normal,
            messages: Vec::new(),
            character_index: 0,
            manual_scroll: 0,
            connection,
            server_addr,
            db_hash: String::from("--------"),
            db_name: String::from("?"),
            branch: String::from("--"),
            history_index: None,
            cursor_line: 0,
            error_popup: None,
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        self.event_loop(&mut terminal)
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            if let Err(e) = terminal.draw(|frame| crate::ui::render(self, frame)) {
                eprintln!("Render error: {}", e);
                continue;
            }

            match crate::input::handle_event(self) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(e) => {
                    eprintln!("Input error: {}", e);
                    continue;
                }
            }
        }
    }

    // Text editing

    pub fn move_cursor_left(&mut self) {
        self.character_index = self.character_index.saturating_sub(1);
    }

    pub fn move_cursor_right(&mut self) {
        let len = self.input_length();
        if self.character_index < len {
            self.character_index += 1;
        }
    }

    pub fn enter_char(&mut self, c: char) {
        self.input.insert(self.byte_index(), c);
        self.move_cursor_right();
    }

    pub fn enter_newline(&mut self) {
        self.input.insert(self.byte_index(), '\n');
        self.character_index += 1;
        self.cursor_line += 1;
    }

    /// Deletes char before cursor (backspace behavior)
    pub fn delete_char(&mut self) {
        if self.character_index == 0 {
            return;
        }

        let byte_idx = self.byte_index();
        // Find the start of the previous character
        let prev_char_start = self.input[..byte_idx]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        let deleted = self.input.as_bytes()[prev_char_start];

        self.input.replace_range(prev_char_start..byte_idx, "");
        self.character_index -= 1;

        if deleted == b'\n' && self.cursor_line > 0 {
            self.cursor_line -= 1;
        }
    }

    pub fn submit_message(&mut self) {
        if self.input.is_empty() {
            return;
        }
        let cmd = std::mem::take(&mut self.input);
        self.character_index = 0;
        self.cursor_line = 0;
        self.manual_scroll = 0;
        self.history_index = None;

        let rendered = self.execute_command(&cmd);
        self.messages.push(QueryEntry {
            input: cmd,
            rendered,
        });
    }

    pub fn history_older(&mut self) {
        if self.messages.is_empty() {
            return;
        }
        let next = match self.history_index {
            None => self.messages.len() - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_index = Some(next);
        self.set_input_from_history(next);
    }

    pub fn history_newer(&mut self) {
        match self.history_index {
            None => {}
            Some(i) if i + 1 >= self.messages.len() => {
                self.history_index = None;
                self.input.clear();
                self.character_index = 0;
                self.cursor_line = 0;
            }
            Some(i) => {
                self.history_index = Some(i + 1);
                self.set_input_from_history(i + 1);
            }
        }
    }

    fn set_input_from_history(&mut self, idx: usize) {
        self.input.clone_from(&self.messages[idx].input);
        self.character_index = self.input.chars().count();
        self.cursor_line = self.input.lines().count().saturating_sub(1);
    }

    pub fn copy_last_result(&self) {
        if let Some(entry) = self.messages.last() {
            let text = format!("{}\n", entry.rendered);
            let _ = arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text));
        }
    }

    pub fn copy_input(&self) {
        if !self.input.is_empty() {
            let _ = arboard::Clipboard::new().and_then(|mut cb| cb.set_text(self.input.clone()));
        }
    }

    fn execute_command(&mut self, cmd: &str) -> String {
        // Collapse multiline input to a single line for the wire protocol.
        let cmd = cmd.split_whitespace().collect::<Vec<_>>().join(" ");

        // Client-side validation: catch malformed S-expressions before sending.
        if let crate::language::InputClassification::MalformedSexp(err) =
            crate::language::classify(&cmd)
        {
            return format!("Syntax error: {}", err);
        }

        // Rewrite client shortcuts (e.g. (schema) -> (drl (Base sakura:attribute))).
        let cmd = crate::language::rewrite(&cmd);

        let result = self.send_or_reconnect(&cmd);
        match result {
            Ok(resp) => self.handle_response(&resp),
            Err(err) => err,
        }
    }

    /// Try to send a command; on failure, attempt to reconnect and retry once.
    fn send_or_reconnect(&mut self, cmd: &str) -> Result<ServerResponse, String> {
        // First attempt
        let first_err = match &mut self.connection {
            Some(conn) => match conn.send(cmd) {
                Ok(resp) => return Ok(resp),
                Err(e) => e.to_string(),
            },
            None => String::from("not connected"),
        };

        // Reconnect and retry
        match connection::Connection::connect(&self.server_addr) {
            Ok(mut conn) => match conn.send(cmd) {
                Ok(resp) => {
                    self.connection = Some(conn);
                    Ok(resp)
                }
                Err(retry_err) => {
                    self.connection = Some(conn);
                    Err(format!(
                        "ERROR: failed to parse server response: {} (retry error: {})",
                        first_err, retry_err
                    ))
                }
            },
            Err(connect_err) => {
                self.connection = None;
                Err(format!(
                    "ERROR: server unreachable ({}): {}",
                    self.server_addr, connect_err
                ))
            }
        }
    }

    /// Process a successful server response: update metadata, show error popup or format output.
    fn handle_response(&mut self, resp: &ServerResponse) -> String {
        let meta = resp.meta();
        self.db_hash.clone_from(&meta.db_hash);
        self.db_name.clone_from(&meta.db_name);
        self.branch.clone_from(&meta.branch);

        if let Some((kind, msg)) = connection::error_parts(resp) {
            self.error_popup = Some((kind.to_string(), msg.to_string()));
            String::new()
        } else {
            format_response(resp)
        }
    }

    // Scrolling

    pub fn scroll_up(&mut self) {
        self.manual_scroll = self.manual_scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.manual_scroll = (self.manual_scroll.saturating_add(1)).min(0);
    }

    /// Calculates scroll position: auto-follows latest message with manual offset
    pub fn calculate_scroll_offset(&self, area_height: u16) -> u16 {
        let total_lines = self.total_content_lines();
        let visible_lines = area_height.saturating_sub(2);
        let auto_scroll = total_lines.saturating_sub(visible_lines);

        self.apply_manual_scroll(auto_scroll)
    }

    pub fn total_content_lines(&self) -> u16 {
        self.messages
            .iter()
            .map(|entry| {
                // "sakura=> {input}\n\n{rendered}\n" + "\n" separator between entries
                let entry_lines = 1 // "sakura=> ..." line(s)
                    + entry.input.lines().count().saturating_sub(1) // extra input lines
                    + 1 // blank line
                    + entry.rendered.lines().count().max(1) // rendered output
                    + 1; // trailing newline
                entry_lines
            })
            .sum::<usize>()
            .saturating_sub(1) as u16 // last entry has no separator
    }

    fn apply_manual_scroll(&self, base: u16) -> u16 {
        ((base as i32) + self.manual_scroll)
            .max(0)
            .try_into()
            .unwrap_or(0)
    }

    // Helpers

    /// Converts character index to byte index for UTF-8 safety
    fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .nth(self.character_index)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    fn input_length(&self) -> usize {
        self.input.chars().count()
    }
}

/// Build the full query results text for display.
pub fn build_query_results(repl: &Repl) -> String {
    let mut out = String::new();
    for (i, entry) in repl.messages.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = write!(out, "sakura=> {}\n\n{}\n", entry.input, entry.rendered);
    }
    out
}
