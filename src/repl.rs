use color_eyre::Result;
use ratatui::DefaultTerminal;

use crate::connection::{self, format_response, Connection};

const SERVER_ADDR: &str = "127.0.0.1:7777";

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
    pub fn new(connection: Option<Connection>) -> Self {
        Self {
            input: String::new(),
            mode: InputMode::Normal,
            messages: Vec::new(),
            character_index: 0,
            manual_scroll: 0,
            connection,
            db_hash: String::from("--------"),
            db_name: String::from("?"),
            branch:  String::from("--"),
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
            // Render with error recovery
            if let Err(e) = terminal.draw(|frame| crate::ui::render(self, frame)) {
                eprintln!("Render error: {}", e);
                continue;
            }

            // Handle input with error recovery
            match crate::input::handle_event(self) {
                Ok(should_quit) if should_quit => return Ok(()),
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Input error: {}", e);
                    continue;
                }
            }
        }
    }

    // Text editing

    pub fn move_cursor_left(&mut self) {
        self.character_index = self
            .character_index
            .saturating_sub(1)
            .clamp(0, self.input_length());
    }

    pub fn move_cursor_right(&mut self) {
        self.character_index = self
            .character_index
            .saturating_add(1)
            .clamp(0, self.input_length());
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
        if self.character_index > 0 {
            let char_to_delete = self.input
                .chars()
                .nth(self.character_index - 1)
                .unwrap_or(' ');

            self.input = self.input_without_char_at(self.character_index - 1);
            self.move_cursor_left();

            // Update cursor_line if deleting a newline
            if char_to_delete == '\n' && self.cursor_line > 0 {
                self.cursor_line -= 1;
            }
        }
    }

    pub fn submit_message(&mut self) {
        if self.input.is_empty() {
            return;
        }
        let cmd = self.input.clone();
        self.input.clear();
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
        self.input = self.messages[next].input.clone();
        self.character_index = self.input.chars().count();
        self.cursor_line = self.input.lines().count().saturating_sub(1);
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
                self.input = self.messages[i + 1].input.clone();
                self.character_index = self.input.chars().count();
                self.cursor_line = self.input.lines().count().saturating_sub(1);
            }
        }
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
        // The server reads one command per line via input_line. Collapse
        // multiline input to a single line so the protocol stays in sync.
        let cmd = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
        let result = if let Some(conn) = &mut self.connection {
            conn.send(&cmd).map_err(|e| e.to_string())
        } else {
            Err(String::from("not connected"))
        };

        match result {
            Ok(resp) => {
                self.db_hash = resp.db_hash().to_string();
                self.db_name = resp.db_name().to_string();
                self.branch  = resp.branch().to_string();
                if let Some((kind, msg)) = connection::error_parts(&resp) {
                    self.error_popup = Some((kind.to_string(), msg.to_string()));
                    String::new()
                } else {
                    format_response(&resp)
                }
            }
            Err(send_err) => {
                // Try to (re)connect and retry once
                match connection::Connection::connect(SERVER_ADDR) {
                    Ok(mut conn) => match conn.send(&cmd) {
                        Ok(resp) => {
                            self.db_hash = resp.db_hash().to_string();
                            self.db_name = resp.db_name().to_string();
                            self.branch  = resp.branch().to_string();
                            let rendered = format_response(&resp);
                            self.connection = Some(conn);
                            rendered
                        }
                        Err(retry_err) => {
                            self.connection = Some(conn);
                            format!("ERROR: failed to parse server response: {} (retry error: {})", send_err, retry_err)
                        }
                    },
                    Err(connect_err) => {
                        self.connection = None;
                        format!("ERROR: server unreachable (127.0.0.1:7777): {}", connect_err)
                    }
                }
            }
        }
    }

    fn input_without_char_at(&self, pos: usize) -> String {
        self.input
            .chars()
            .enumerate()
            .filter(|(i, _)| *i != pos)
            .map(|(_, c)| c)
            .collect()
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
            .map(|entry| format!("sakura=> {}\n\n{}\n", entry.input, entry.rendered))
            .collect::<Vec<_>>()
            .join("\n")
            .lines()
            .count() as u16
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
