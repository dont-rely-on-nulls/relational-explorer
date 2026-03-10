use color_eyre::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use crate::repl::{InputMode, Repl};

/// Processes input events and returns true if the REPL should exit
pub fn handle_event(repl: &mut Repl) -> Result<bool> {
    let event = event::read()?;
    dispatch_event(repl, event)
}

fn dispatch_event(repl: &mut Repl, event: Event) -> Result<bool> {
    match event {
        Event::Key(key) if should_handle_key(repl.mode, key.kind) => {
            handle_key(repl, key.code, key.modifiers)
        }
        Event::Mouse(mouse) => Ok(handle_mouse(repl, mouse.kind)),
        _ => Ok(false),
    }
}

fn should_handle_key(mode: InputMode, kind: KeyEventKind) -> bool {
    matches!(mode, InputMode::Normal) || kind == KeyEventKind::Press
}

fn handle_key(repl: &mut Repl, key: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
    match repl.mode {
        InputMode::Normal => handle_normal_mode(repl, key),
        InputMode::Editing => handle_editing_mode(repl, key, modifiers),
    }
}

fn handle_normal_mode(repl: &mut Repl, key: KeyCode) -> Result<bool> {
    let should_quit = matches!(key, KeyCode::Char('q'));

    use KeyCode::*;
    match key {
        Char('e') => repl.mode = InputMode::Editing,
        Char('y') => repl.copy_last_result(),
        Up => repl.scroll_up(),
        Down => repl.scroll_down(),
        _ => {}
    }

    Ok(should_quit)
}

fn handle_editing_mode(repl: &mut Repl, key: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
    use KeyCode::*;
    match key {
        Esc => repl.mode = InputMode::Normal,
        Enter => repl.submit_message(),
        Backspace => repl.delete_char(),
        Left => repl.move_cursor_left(),
        Right => repl.move_cursor_right(),
        Up => repl.history_older(),
        Down => repl.history_newer(),
        Char('y') if modifiers.contains(KeyModifiers::CONTROL) => repl.copy_input(),
        Char(c) => repl.enter_char(c),
        _ => {}
    }

    Ok(false)
}

fn handle_mouse(repl: &mut Repl, kind: MouseEventKind) -> bool {
    use MouseEventKind::*;
    match kind {
        ScrollUp => repl.scroll_up(),
        ScrollDown => repl.scroll_down(),
        _ => {}
    }
    false
}
