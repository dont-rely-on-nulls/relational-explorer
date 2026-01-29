use color_eyre::Result;
use ratatui::DefaultTerminal;

const LINES_PER_QUERY: u16 = 12;

/// REPL session state
pub struct Repl {
    pub input: String,
    pub character_index: usize,
    pub mode: InputMode,
    pub messages: Vec<String>,
    /// Offset from auto-scroll position (negative = scrolled up into history)
    pub manual_scroll: i32,
}

#[derive(Clone, Copy)]
pub enum InputMode {
    Normal,
    Editing,
}

impl Repl {
    pub const fn new() -> Self {
        Self {
            input: String::new(),
            mode: InputMode::Normal,
            messages: Vec::new(),
            character_index: 0,
            manual_scroll: 0,
        }
    }

    /// Main event loop with mouse support
    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        Self::enable_mouse_capture()?;
        let result = self.event_loop(&mut terminal);
        Self::disable_mouse_capture()?;
        result
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| crate::ui::render(self, frame))?;

            if crate::input::handle_event(self)? {
                return Ok(());
            }
        }
    }

    fn enable_mouse_capture() -> Result<()> {
        ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::EnableMouseCapture
        )?;
        Ok(())
    }

    fn disable_mouse_capture() -> Result<()> {
        ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::DisableMouseCapture
        )?;
        Ok(())
    }

    // Text editing

    pub fn move_cursor_left(&mut self) {
        self.character_index = self.character_index
            .saturating_sub(1)
            .clamp(0, self.input_length());
    }

    pub fn move_cursor_right(&mut self) {
        self.character_index = self.character_index
            .saturating_add(1)
            .clamp(0, self.input_length());
    }

    pub fn enter_char(&mut self, c: char) {
        self.input.insert(self.byte_index(), c);
        self.move_cursor_right();
    }

    /// Deletes char before cursor (backspace behavior)
    pub fn delete_char(&mut self) {
        if self.character_index > 0 {
            self.input = self.input_without_char_at(self.character_index - 1);
            self.move_cursor_left();
        }
    }

    pub fn submit_message(&mut self) {
        if !self.input.is_empty() {
            self.messages.push(self.input.clone());
            self.input.clear();
            self.character_index = 0;
            self.manual_scroll = 0;
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

    fn total_content_lines(&self) -> u16 {
        (self.messages.len() as u16) * LINES_PER_QUERY
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
