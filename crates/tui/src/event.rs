use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use openprofiler_core::model::ViewId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Filter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HandleResult {
    Continue,
    Quit,
}

pub struct KeyEventResult {
    pub should_quit: bool,
    pub input_mode: InputMode,
}

impl KeyEventResult {
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }
}

pub fn handle_key(app: &mut super::app::App, key: KeyEvent) -> KeyEventResult {
    let mut result = KeyEventResult {
        should_quit: false,
        input_mode: app.input_mode,
    };

    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return result;
    }

    match app.input_mode {
        InputMode::Filter => handle_filter_input(app, key, &mut result),
        InputMode::Normal => handle_normal_input(app, key, &mut result),
    }

    result
}

fn handle_filter_input(app: &mut super::app::App, key: KeyEvent, result: &mut KeyEventResult) {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            result.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
            result.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            app.filter_input.pop();
        }
        KeyCode::Char(c) => {
            app.filter_input.push(c);
        }
        _ => {}
    }
}

fn handle_normal_input(app: &mut super::app::App, key: KeyEvent, result: &mut KeyEventResult) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            if key.modifiers.contains(KeyModifiers::NONE) {
                result.should_quit = true;
            }
        }
        KeyCode::Tab => {
            app.toggle_focus_pane();
        }
        KeyCode::BackTab => {
            app.toggle_focus_pane();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.navigate_down();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.navigate_up();
        }
        KeyCode::PageDown => {
            app.page_down();
        }
        KeyCode::PageUp => {
            app.page_up();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.navigate_left();
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.navigate_right();
        }
        KeyCode::Enter => {
            app.select_current();
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.refresh_current();
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            app.toggle_recording();
        }
        KeyCode::Char('g') | KeyCode::Char('G') => {
            app.run_gc();
        }
        KeyCode::Char('m') | KeyCode::Char('M') => {
            app.mark_heap();
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            app.copy_selected_item();
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            app.enter_filter_mode();
            result.input_mode = InputMode::Filter;
        }
        KeyCode::Char('/') => {
            app.enter_filter_mode();
            result.input_mode = InputMode::Filter;
        }
        KeyCode::Char('1') => app.go_to_view(ViewId::TeleOverview),
        KeyCode::Char('2') => app.go_to_view(ViewId::CpuHotSpots),
        KeyCode::Char('3') => app.go_to_view(ViewId::LiveAllocationHotSpots),
        KeyCode::Char('4') => app.go_to_view(ViewId::DatabasesJdbc),
        _ => {}
    }
}
