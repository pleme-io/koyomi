//! Keyboard input handling with vim-style navigation.
//!
//! Handles key events and translates them into calendar actions
//! based on the current mode (Normal, EventEditor, Command).
//!
//! Key binding definitions use awase types for platform-agnostic hotkey
//! representation and serializable binding configuration.

use awase::{Hotkey, Key as AwaseKey, Modifiers as AwaseMods};
use madori::event::{KeyCode, KeyEvent};

/// A keybinding definition: an awase `Hotkey` paired with an action name.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyBinding {
    /// The hotkey that triggers this binding (awase type).
    pub hotkey: Hotkey,
    /// The action name to perform.
    pub action: String,
}

/// Default keybindings using awase `Hotkey` types.
#[must_use]
pub fn default_bindings() -> Vec<KeyBinding> {
    vec![
        // Vim navigation
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::J), action: "move_forward".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::K), action: "move_backward".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::H), action: "prev_week".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::L), action: "next_week".into() },
        // Month navigation
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::N), action: "next_month".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::P), action: "prev_month".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::SHIFT, AwaseKey::L), action: "next_month".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::SHIFT, AwaseKey::H), action: "prev_month".into() },
        // Jump to today
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::T), action: "jump_today".into() },
        // View switching
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::V), action: "cycle_view".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::Num1), action: "month_view".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::Num2), action: "week_view".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::Num3), action: "day_view".into() },
        // Event operations
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::A), action: "add_event".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::E), action: "edit_event".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::D), action: "delete_event".into() },
        // Search
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::Slash), action: "search".into() },
        // Quit
        KeyBinding { hotkey: Hotkey::new(AwaseMods::NONE, AwaseKey::Q), action: "quit".into() },
    ]
}

/// Application input mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal mode — navigate the calendar grid.
    Normal,
    /// Event editor mode — creating or editing an event.
    EventEditor,
    /// Command mode — typing a command after `:`.
    Command,
}

/// Actions that can be triggered by keyboard input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    // Navigation
    /// Move cursor forward (j in month = next day, in week/day = next time slot).
    MoveForward,
    /// Move cursor backward (k).
    MoveBackward,
    /// Move to previous week/column (h in month view).
    MovePrevWeek,
    /// Move to next week/column (l in month view).
    MoveNextWeek,
    /// Go to previous month (H / p).
    PrevMonth,
    /// Go to next month (L / n).
    NextMonth,
    /// Jump to today (t).
    JumpToday,

    // View switching
    /// Cycle to next view (v).
    CycleView,
    /// Switch to month view (1).
    MonthView,
    /// Switch to week view (2).
    WeekView,
    /// Switch to day view (3).
    DayView,

    // Event operations
    /// Add a new event (a).
    AddEvent,
    /// Edit the selected event (e).
    EditEvent,
    /// Delete the selected event (d).
    DeleteEvent,
    /// Open day detail from month view (Enter).
    OpenDay,

    // Event editor
    /// Cycle to next field in event editor (Tab).
    NextField,
    /// Save the current event (Enter in editor).
    SaveEvent,
    /// Cancel event editing (Esc in editor).
    CancelEdit,
    /// Type a character in the current field.
    TypeChar(char),
    /// Backspace in editor.
    Backspace,

    // Command mode
    /// Enter command mode (:).
    EnterCommand,
    /// Submit the command (Enter in command mode).
    SubmitCommand,
    /// Cancel command mode (Esc).
    CancelCommand,

    // Search
    /// Start search (/).
    Search,

    // Application
    /// Quit the application (q).
    Quit,

    /// No action for this key.
    None,
}

/// Translate a key event into an action based on the current mode.
#[must_use]
pub fn handle_key(event: &KeyEvent, mode: InputMode) -> Action {
    if !event.pressed {
        return Action::None;
    }

    match mode {
        InputMode::Normal => handle_normal_key(event),
        InputMode::EventEditor => handle_editor_key(event),
        InputMode::Command => handle_command_key(event),
    }
}

fn handle_normal_key(event: &KeyEvent) -> Action {
    // Check for modified keys first
    if event.modifiers.shift {
        match event.key {
            KeyCode::Char('H') => return Action::PrevMonth,
            KeyCode::Char('L') => return Action::NextMonth,
            _ => {}
        }
    }

    match event.key {
        // Vim navigation
        KeyCode::Char('j') | KeyCode::Down => Action::MoveForward,
        KeyCode::Char('k') | KeyCode::Up => Action::MoveBackward,
        KeyCode::Char('h') | KeyCode::Left => Action::MovePrevWeek,
        KeyCode::Char('l') | KeyCode::Right => Action::MoveNextWeek,

        // Month navigation
        KeyCode::Char('n') => Action::NextMonth,
        KeyCode::Char('p') => Action::PrevMonth,

        // Jump to today
        KeyCode::Char('t') => Action::JumpToday,

        // View switching
        KeyCode::Char('v') => Action::CycleView,
        KeyCode::Char('1') => Action::MonthView,
        KeyCode::Char('2') => Action::WeekView,
        KeyCode::Char('3') => Action::DayView,

        // Event operations
        KeyCode::Char('a') => Action::AddEvent,
        KeyCode::Char('e') => Action::EditEvent,
        KeyCode::Char('d') => Action::DeleteEvent,
        KeyCode::Enter => Action::OpenDay,

        // Command/search
        KeyCode::Char(':') => Action::EnterCommand,
        KeyCode::Char('/') => Action::Search,

        // Quit
        KeyCode::Char('q') => Action::Quit,

        _ => Action::None,
    }
}

fn handle_editor_key(event: &KeyEvent) -> Action {
    match event.key {
        KeyCode::Tab => Action::NextField,
        KeyCode::Enter => Action::SaveEvent,
        KeyCode::Escape => Action::CancelEdit,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Char(c) => Action::TypeChar(c),
        _ => Action::None,
    }
}

fn handle_command_key(event: &KeyEvent) -> Action {
    match event.key {
        KeyCode::Enter => Action::SubmitCommand,
        KeyCode::Escape => Action::CancelCommand,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Char(c) => Action::TypeChar(c),
        _ => Action::None,
    }
}

/// Editor fields for event creation/editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorField {
    Title,
    StartDate,
    StartTime,
    EndDate,
    EndTime,
    Location,
    Calendar,
    Recurrence,
}

impl EditorField {
    /// Cycle to the next field.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Title => Self::StartDate,
            Self::StartDate => Self::StartTime,
            Self::StartTime => Self::EndDate,
            Self::EndDate => Self::EndTime,
            Self::EndTime => Self::Location,
            Self::Location => Self::Calendar,
            Self::Calendar => Self::Recurrence,
            Self::Recurrence => Self::Title,
        }
    }

    /// Display label for this field.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::StartDate => "Start Date",
            Self::StartTime => "Start Time",
            Self::EndDate => "End Date",
            Self::EndTime => "End Time",
            Self::Location => "Location",
            Self::Calendar => "Calendar",
            Self::Recurrence => "Recurrence",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use madori::event::Modifiers;

    #[test]
    fn default_bindings_are_valid() {
        let bindings = default_bindings();
        assert!(!bindings.is_empty());
        let has_quit = bindings.iter().any(|b| b.action == "quit");
        assert!(has_quit, "should have a quit binding");
    }

    #[test]
    fn bindings_are_serializable() {
        let bindings = default_bindings();
        let json = serde_json::to_string(&bindings).unwrap();
        let deserialized: Vec<KeyBinding> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.len(), bindings.len());
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            key: code,
            pressed: true,
            modifiers: Modifiers::default(),
            text: None,
        }
    }

    fn shifted_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            key: code,
            pressed: true,
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            text: None,
        }
    }

    fn released_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            key: code,
            pressed: false,
            modifiers: Modifiers::default(),
            text: None,
        }
    }

    #[test]
    fn released_keys_produce_no_action() {
        assert_eq!(handle_key(&released_key(KeyCode::Char('j')), InputMode::Normal), Action::None);
    }

    #[test]
    fn normal_vim_navigation() {
        assert_eq!(handle_key(&key(KeyCode::Char('j')), InputMode::Normal), Action::MoveForward);
        assert_eq!(handle_key(&key(KeyCode::Char('k')), InputMode::Normal), Action::MoveBackward);
        assert_eq!(handle_key(&key(KeyCode::Char('h')), InputMode::Normal), Action::MovePrevWeek);
        assert_eq!(handle_key(&key(KeyCode::Char('l')), InputMode::Normal), Action::MoveNextWeek);
    }

    #[test]
    fn normal_arrow_navigation() {
        assert_eq!(handle_key(&key(KeyCode::Down), InputMode::Normal), Action::MoveForward);
        assert_eq!(handle_key(&key(KeyCode::Up), InputMode::Normal), Action::MoveBackward);
        assert_eq!(handle_key(&key(KeyCode::Left), InputMode::Normal), Action::MovePrevWeek);
        assert_eq!(handle_key(&key(KeyCode::Right), InputMode::Normal), Action::MoveNextWeek);
    }

    #[test]
    fn normal_month_navigation() {
        assert_eq!(handle_key(&key(KeyCode::Char('n')), InputMode::Normal), Action::NextMonth);
        assert_eq!(handle_key(&key(KeyCode::Char('p')), InputMode::Normal), Action::PrevMonth);
        assert_eq!(handle_key(&shifted_key(KeyCode::Char('L')), InputMode::Normal), Action::NextMonth);
        assert_eq!(handle_key(&shifted_key(KeyCode::Char('H')), InputMode::Normal), Action::PrevMonth);
    }

    #[test]
    fn normal_view_switching() {
        assert_eq!(handle_key(&key(KeyCode::Char('v')), InputMode::Normal), Action::CycleView);
        assert_eq!(handle_key(&key(KeyCode::Char('1')), InputMode::Normal), Action::MonthView);
        assert_eq!(handle_key(&key(KeyCode::Char('2')), InputMode::Normal), Action::WeekView);
        assert_eq!(handle_key(&key(KeyCode::Char('3')), InputMode::Normal), Action::DayView);
    }

    #[test]
    fn normal_event_operations() {
        assert_eq!(handle_key(&key(KeyCode::Char('a')), InputMode::Normal), Action::AddEvent);
        assert_eq!(handle_key(&key(KeyCode::Char('e')), InputMode::Normal), Action::EditEvent);
        assert_eq!(handle_key(&key(KeyCode::Char('d')), InputMode::Normal), Action::DeleteEvent);
    }

    #[test]
    fn editor_mode() {
        assert_eq!(handle_key(&key(KeyCode::Tab), InputMode::EventEditor), Action::NextField);
        assert_eq!(handle_key(&key(KeyCode::Enter), InputMode::EventEditor), Action::SaveEvent);
        assert_eq!(handle_key(&key(KeyCode::Escape), InputMode::EventEditor), Action::CancelEdit);
        assert_eq!(handle_key(&key(KeyCode::Char('x')), InputMode::EventEditor), Action::TypeChar('x'));
    }

    #[test]
    fn command_mode() {
        assert_eq!(handle_key(&key(KeyCode::Enter), InputMode::Command), Action::SubmitCommand);
        assert_eq!(handle_key(&key(KeyCode::Escape), InputMode::Command), Action::CancelCommand);
        assert_eq!(handle_key(&key(KeyCode::Char('a')), InputMode::Command), Action::TypeChar('a'));
    }

    #[test]
    fn editor_field_cycling() {
        let mut field = EditorField::Title;
        field = field.next();
        assert_eq!(field, EditorField::StartDate);
        field = field.next();
        assert_eq!(field, EditorField::StartTime);
        // Full cycle back to Title
        for _ in 0..6 {
            field = field.next();
        }
        assert_eq!(field, EditorField::Title);
    }
}
