//! Koyomi (暦) — GPU-rendered calendar application.
//!
//! Supports month, week, and day views with vim-style navigation,
//! local YAML event storage, recurring events, and reminders.

mod calendar;
mod config;
mod events;
mod input;
mod platform;
mod recurrence;
mod reminder;
mod render;

use std::sync::{Arc, Mutex};

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::calendar::{ViewMode, WeekStart};
use crate::config::KoyomiConfig;
use crate::events::{Event, EventStore};
use crate::input::{Action, InputMode};
use crate::render::{AppState, EditorState, KoyomiRenderer};

#[derive(Parser)]
#[command(name = "koyomi", about = "Koyomi (暦) — GPU calendar app")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show today's events.
    Today,
    /// Show this week's events.
    Week,
    /// Add a new event.
    Add {
        /// Event title.
        title: String,
        /// Start time (ISO 8601: YYYY-MM-DDTHH:MM).
        #[arg(long)]
        start: String,
        /// End time (ISO 8601: YYYY-MM-DDTHH:MM).
        #[arg(long)]
        end: String,
        /// Event location.
        #[arg(long)]
        location: Option<String>,
        /// Calendar name.
        #[arg(long, default_value = "default")]
        calendar: String,
    },
    /// Delete an event by ID.
    Delete {
        /// Event ID.
        id: String,
    },
    /// List all events.
    List,
    /// Run the background reminder daemon.
    Daemon,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load config via shikumi
    let config = load_config();

    // Open event store
    let store = EventStore::open_default().unwrap_or_else(|e| {
        tracing::error!("failed to open event store: {e}");
        std::process::exit(1);
    });

    match cli.command {
        Some(Command::Today) => cmd_today(&store, &config),
        Some(Command::Week) => cmd_week(&store, &config),
        Some(Command::Add {
            title,
            start,
            end,
            location,
            calendar,
        }) => cmd_add(&store, &config, &title, &start, &end, location.as_deref(), &calendar),
        Some(Command::Delete { id }) => cmd_delete(&store, &id),
        Some(Command::List) => cmd_list(&store),
        Some(Command::Daemon) => cmd_daemon(&store, &config),
        None => launch_gui(config, store),
    }
}

fn load_config() -> KoyomiConfig {
    match shikumi::ConfigDiscovery::new("koyomi")
        .env_override("KOYOMI_CONFIG")
        .discover()
    {
        Ok(path) => {
            tracing::info!("loading config from {}", path.display());
            let store_result = shikumi::ConfigStore::<KoyomiConfig>::load(&path, "KOYOMI_");
            match store_result {
                Ok(store) => KoyomiConfig::clone(&store.get()),
                Err(e) => {
                    tracing::warn!("failed to load config: {e}, using defaults");
                    KoyomiConfig::default()
                }
            }
        }
        Err(_) => {
            tracing::info!("no config file found, using defaults");
            KoyomiConfig::default()
        }
    }
}

fn parse_datetime(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .unwrap_or_else(|_| {
            eprintln!("Invalid datetime format: {s} (expected YYYY-MM-DDTHH:MM)");
            std::process::exit(1);
        })
}

fn cmd_today(store: &EventStore, config: &KoyomiConfig) {
    let today = Local::now().date_naive();
    let use_24h = config.appearance.time_format == "24h";
    match store.query_date(today) {
        Ok(events) => {
            if events.is_empty() {
                println!("No events today ({}).", today.format("%A, %B %d"));
            } else {
                println!("Events for {} ({}):", today.format("%A, %B %d"), today);
                for occ in &events {
                    let time_fmt = if use_24h { "%H:%M" } else { "%I:%M %p" };
                    println!(
                        "  {} - {} | {} [{}]",
                        occ.occurrence_start.format(time_fmt),
                        occ.occurrence_end.format(time_fmt),
                        occ.event.title,
                        occ.event.calendar,
                    );
                    if let Some(ref loc) = occ.event.location {
                        println!("    Location: {loc}");
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error querying events: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_week(store: &EventStore, config: &KoyomiConfig) {
    let today = Local::now().date_naive();
    let week_start = WeekStart::from_str(&config.appearance.week_start);
    let dates = calendar::week_dates(today, week_start);
    let use_24h = config.appearance.time_format == "24h";

    let start_dt = dates[0].and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    let end_dt = dates[6].and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
        + Duration::seconds(1);

    match store.query_range(start_dt, end_dt) {
        Ok(events) => {
            if events.is_empty() {
                println!(
                    "No events this week ({} - {}).",
                    dates[0].format("%b %d"),
                    dates[6].format("%b %d")
                );
            } else {
                println!(
                    "Events for week of {} - {}:",
                    dates[0].format("%b %d"),
                    dates[6].format("%b %d, %Y")
                );
                let mut current_date = None;
                for occ in &events {
                    let occ_date = occ.occurrence_start.date();
                    if current_date != Some(occ_date) {
                        current_date = Some(occ_date);
                        println!("\n  {}:", occ_date.format("%A, %B %d"));
                    }
                    let time_fmt = if use_24h { "%H:%M" } else { "%I:%M %p" };
                    println!(
                        "    {} - {} | {} [{}]",
                        occ.occurrence_start.format(time_fmt),
                        occ.occurrence_end.format(time_fmt),
                        occ.event.title,
                        occ.event.calendar,
                    );
                }
                println!();
            }
        }
        Err(e) => {
            eprintln!("Error querying events: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_add(
    store: &EventStore,
    config: &KoyomiConfig,
    title: &str,
    start: &str,
    end: &str,
    location: Option<&str>,
    calendar: &str,
) {
    let start_dt = parse_datetime(start);
    let end_dt = parse_datetime(end);

    let mut event = Event::new(title.to_string(), start_dt, end_dt);
    event.location = location.map(ToString::to_string);
    event.calendar = calendar.to_string();
    event.reminders = vec![config.notifications.default_reminder_mins];

    match store.create(&event) {
        Ok(()) => {
            println!("Event created: {} ({})", event.title, event.id);
            println!(
                "  {} - {}",
                event.start.format("%Y-%m-%d %H:%M"),
                event.end.format("%H:%M")
            );
        }
        Err(e) => {
            eprintln!("Error creating event: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_delete(store: &EventStore, id: &str) {
    match store.delete(id) {
        Ok(()) => println!("Event deleted: {id}"),
        Err(e) => {
            eprintln!("Error deleting event: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_list(store: &EventStore) {
    match store.list_all() {
        Ok(events) => {
            if events.is_empty() {
                println!("No events stored.");
            } else {
                println!("{} events:", events.len());
                for event in &events {
                    println!(
                        "  [{}] {} | {} - {} [{}]{}",
                        &event.id[..8],
                        event.title,
                        event.start.format("%Y-%m-%d %H:%M"),
                        event.end.format("%H:%M"),
                        event.calendar,
                        if event.recurrence.is_some() {
                            " (recurring)"
                        } else {
                            ""
                        },
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Error listing events: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_daemon(store: &EventStore, config: &KoyomiConfig) {
    if !config.notifications.enabled {
        tracing::info!("notifications disabled in config, daemon has nothing to do");
        return;
    }

    tracing::info!("starting reminder daemon (checking every 60s)");
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let mut scheduler = reminder::ReminderScheduler::new();
        loop {
            match scheduler.check(store) {
                Ok(notifications) => {
                    for notif in &notifications {
                        println!("{notif}");
                        tracing::info!("{notif}");
                    }
                }
                Err(e) => tracing::warn!("reminder check failed: {e}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    });
}

fn launch_gui(config: KoyomiConfig, store: EventStore) {
    let today = Local::now().date_naive();
    let week_start = WeekStart::from_str(&config.appearance.week_start);
    let use_24h = config.appearance.time_format == "24h";
    let font_size = config.appearance.font_size;

    // Load initial events for the current month
    let month_start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    let month_end = month_start + Duration::days(42); // 6 weeks
    let cached_events = store.query_range(month_start, month_end).unwrap_or_default();

    let state = Arc::new(Mutex::new(AppState {
        view_mode: ViewMode::Month,
        cursor_date: today,
        year: today.year(),
        month: today.month(),
        week_start,
        use_24h,
        input_mode: InputMode::Normal,
        store,
        status: String::new(),
        command_buffer: String::new(),
        editor: None,
        font_size,
        selected_hour: 9,
        selected_event_idx: 0,
        cached_events,
        cache_dirty: false,
    }));

    let renderer = KoyomiRenderer::new(state.clone());

    let event_state = state.clone();
    madori::App::builder(renderer)
        .title("Koyomi 暦")
        .size(config.appearance.width, config.appearance.height)
        .on_event(move |event, _renderer| -> madori::EventResponse {
            use madori::event::AppEvent;

            let AppEvent::Key(key_event) = event else {
                return madori::EventResponse::default();
            };

            let mut state = event_state.lock().unwrap();
            let action = input::handle_key(key_event, state.input_mode);

            match action {
                Action::None => return madori::EventResponse::default(),
                Action::Quit => {
                    return madori::EventResponse {
                        exit: true,
                        consumed: true,
                        ..Default::default()
                    };
                }

                // Navigation
                Action::MoveForward => match state.view_mode {
                    ViewMode::Month => {
                        state.cursor_date += Duration::days(1);
                        update_month_if_needed(&mut state);
                    }
                    ViewMode::Week | ViewMode::Day => {
                        state.selected_hour = (state.selected_hour + 1).min(23);
                    }
                },
                Action::MoveBackward => match state.view_mode {
                    ViewMode::Month => {
                        state.cursor_date -= Duration::days(1);
                        update_month_if_needed(&mut state);
                    }
                    ViewMode::Week | ViewMode::Day => {
                        state.selected_hour = state.selected_hour.saturating_sub(1);
                    }
                },
                Action::MovePrevWeek => match state.view_mode {
                    ViewMode::Month => {
                        state.cursor_date -= Duration::days(7);
                        update_month_if_needed(&mut state);
                    }
                    ViewMode::Week => {
                        state.cursor_date -= Duration::days(1);
                    }
                    ViewMode::Day => {
                        state.cursor_date -= Duration::days(1);
                        refresh_cache(&mut state);
                    }
                },
                Action::MoveNextWeek => match state.view_mode {
                    ViewMode::Month => {
                        state.cursor_date += Duration::days(7);
                        update_month_if_needed(&mut state);
                    }
                    ViewMode::Week => {
                        state.cursor_date += Duration::days(1);
                    }
                    ViewMode::Day => {
                        state.cursor_date += Duration::days(1);
                        refresh_cache(&mut state);
                    }
                },
                Action::PrevMonth => {
                    let (y, m) = calendar::prev_month(state.year, state.month);
                    state.year = y;
                    state.month = m;
                    state.cursor_date =
                        NaiveDate::from_ymd_opt(y, m, 1).unwrap();
                    refresh_cache(&mut state);
                }
                Action::NextMonth => {
                    let (y, m) = calendar::next_month(state.year, state.month);
                    state.year = y;
                    state.month = m;
                    state.cursor_date =
                        NaiveDate::from_ymd_opt(y, m, 1).unwrap();
                    refresh_cache(&mut state);
                }
                Action::JumpToday => {
                    let today = Local::now().date_naive();
                    state.cursor_date = today;
                    state.year = today.year();
                    state.month = today.month();
                    refresh_cache(&mut state);
                }

                // View switching
                Action::CycleView => {
                    state.view_mode = state.view_mode.next();
                    refresh_cache(&mut state);
                }
                Action::MonthView => {
                    state.view_mode = ViewMode::Month;
                    refresh_cache(&mut state);
                }
                Action::WeekView => {
                    state.view_mode = ViewMode::Week;
                    refresh_cache(&mut state);
                }
                Action::DayView => {
                    state.view_mode = ViewMode::Day;
                    refresh_cache(&mut state);
                }

                // Open day detail
                Action::OpenDay => {
                    if state.view_mode == ViewMode::Month {
                        state.view_mode = ViewMode::Day;
                        refresh_cache(&mut state);
                    }
                }

                // Event operations
                Action::AddEvent => {
                    let editor = EditorState::new_for_date(
                        state.cursor_date,
                        state.selected_hour,
                    );
                    state.editor = Some(editor);
                    state.input_mode = InputMode::EventEditor;
                }
                Action::EditEvent => {
                    // Find event at cursor
                    let events_on_day: Vec<_> = state
                        .cached_events
                        .iter()
                        .filter(|occ| occ.occurrence_start.date() == state.cursor_date)
                        .collect();
                    if let Some(occ) = events_on_day.get(state.selected_event_idx) {
                        let editor = EditorState::from_event(&occ.event);
                        state.editor = Some(editor);
                        state.input_mode = InputMode::EventEditor;
                    } else {
                        state.status = "No event selected".to_string();
                    }
                }
                Action::DeleteEvent => {
                    let events_on_day: Vec<_> = state
                        .cached_events
                        .iter()
                        .filter(|occ| occ.occurrence_start.date() == state.cursor_date)
                        .collect();
                    if let Some(occ) = events_on_day.get(state.selected_event_idx) {
                        let id = occ.event.id.clone();
                        let title = occ.event.title.clone();
                        match state.store.delete(&id) {
                            Ok(()) => {
                                state.status = format!("Deleted: {title}");
                                refresh_cache(&mut state);
                            }
                            Err(e) => {
                                state.status = format!("Delete failed: {e}");
                            }
                        }
                    } else {
                        state.status = "No event selected".to_string();
                    }
                }

                // Event editor
                Action::NextField => {
                    if let Some(ref mut editor) = state.editor {
                        editor.active_field = editor.active_field.next();
                    }
                }
                Action::SaveEvent => {
                    if let Some(editor) = state.editor.take() {
                        match save_from_editor(&editor, &state.store, &state) {
                            Ok(msg) => {
                                state.status = msg;
                                state.input_mode = InputMode::Normal;
                                refresh_cache(&mut state);
                            }
                            Err(e) => {
                                state.status = format!("Save failed: {e}");
                                state.editor = Some(editor);
                            }
                        }
                    }
                }
                Action::CancelEdit => {
                    state.editor = None;
                    state.input_mode = InputMode::Normal;
                    state.status.clear();
                }
                Action::TypeChar(c) => {
                    if state.input_mode == InputMode::EventEditor {
                        if let Some(ref mut editor) = state.editor {
                            editor.active_value_mut().push(c);
                        }
                    } else if state.input_mode == InputMode::Command {
                        state.command_buffer.push(c);
                    }
                }
                Action::Backspace => {
                    if state.input_mode == InputMode::EventEditor {
                        if let Some(ref mut editor) = state.editor {
                            editor.active_value_mut().pop();
                        }
                    } else if state.input_mode == InputMode::Command {
                        state.command_buffer.pop();
                    }
                }

                // Command mode
                Action::EnterCommand => {
                    state.input_mode = InputMode::Command;
                    state.command_buffer.clear();
                }
                Action::SubmitCommand => {
                    let cmd = state.command_buffer.clone();
                    state.input_mode = InputMode::Normal;
                    state.command_buffer.clear();
                    execute_command(&cmd, &mut state);
                }
                Action::CancelCommand => {
                    state.input_mode = InputMode::Normal;
                    state.command_buffer.clear();
                }

                // Search (simplified: just enter command mode with search prefix)
                Action::Search => {
                    state.input_mode = InputMode::Command;
                    state.command_buffer = "search ".to_string();
                }
            }

            madori::EventResponse::consumed()
        })
        .run()
        .unwrap_or_else(|e| {
            tracing::error!("GUI error: {e}");
            std::process::exit(1);
        });
}

/// Update the displayed month to match the cursor date if it moved out of range.
fn update_month_if_needed(state: &mut AppState) {
    let d = state.cursor_date;
    if d.year() != state.year || d.month() != state.month {
        state.year = d.year();
        state.month = d.month();
        refresh_cache(state);
    }
}

/// Refresh the event cache for the current view.
fn refresh_cache(state: &mut AppState) {
    let (range_start, range_end) = match state.view_mode {
        ViewMode::Month => {
            // Load 6 weeks around the first of the month
            let start = NaiveDate::from_ymd_opt(state.year, state.month, 1)
                .unwrap()
                .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
                - Duration::days(7);
            let end = start + Duration::days(49);
            (start, end)
        }
        ViewMode::Week => {
            let dates = calendar::week_dates(state.cursor_date, state.week_start);
            let start = dates[0].and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
            let end = dates[6].and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
                + Duration::seconds(1);
            (start, end)
        }
        ViewMode::Day => {
            let start = state
                .cursor_date
                .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
            let end = start + Duration::days(1);
            (start, end)
        }
    };

    state.cached_events = state
        .store
        .query_range(range_start, range_end)
        .unwrap_or_default();
    state.selected_event_idx = 0;
}

/// Save an event from the editor state.
fn save_from_editor(
    editor: &EditorState,
    store: &EventStore,
    _state: &AppState,
) -> Result<String, String> {
    if editor.title.is_empty() {
        return Err("Title is required".to_string());
    }

    let start_dt = NaiveDateTime::parse_from_str(
        &format!("{}T{}", editor.start_date, editor.start_time),
        "%Y-%m-%dT%H:%M",
    )
    .map_err(|e| format!("Invalid start: {e}"))?;

    let end_dt = NaiveDateTime::parse_from_str(
        &format!("{}T{}", editor.end_date, editor.end_time),
        "%Y-%m-%dT%H:%M",
    )
    .map_err(|e| format!("Invalid end: {e}"))?;

    if end_dt <= start_dt {
        return Err("End time must be after start time".to_string());
    }

    // Parse recurrence if specified
    let recurrence = if editor.recurrence.is_empty() {
        None
    } else {
        let freq = match editor.recurrence.to_lowercase().as_str() {
            "daily" => Some(recurrence::Frequency::Daily),
            "weekly" => Some(recurrence::Frequency::Weekly),
            "monthly" => Some(recurrence::Frequency::Monthly),
            "yearly" => Some(recurrence::Frequency::Yearly),
            _ => None,
        };
        freq.map(|f| recurrence::RecurrenceRule {
            freq: f,
            ..Default::default()
        })
    };

    if let Some(ref id) = editor.editing_id {
        // Update existing event
        let mut event = store.read(id).map_err(|e| format!("Read failed: {e}"))?;
        event.title.clone_from(&editor.title);
        event.start = start_dt;
        event.end = end_dt;
        event.location = if editor.location.is_empty() {
            None
        } else {
            Some(editor.location.clone())
        };
        event.calendar.clone_from(&editor.calendar);
        event.recurrence = recurrence;
        store.update(&event).map_err(|e| format!("Update failed: {e}"))?;
        Ok(format!("Updated: {}", event.title))
    } else {
        // Create new event
        let mut event = Event::new(editor.title.clone(), start_dt, end_dt);
        event.location = if editor.location.is_empty() {
            None
        } else {
            Some(editor.location.clone())
        };
        event.calendar.clone_from(&editor.calendar);
        event.recurrence = recurrence;
        store.create(&event).map_err(|e| format!("Create failed: {e}"))?;
        Ok(format!("Created: {}", event.title))
    }
}

/// Execute a command string.
fn execute_command(cmd: &str, state: &mut AppState) {
    let parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
    match parts.first().copied() {
        Some("view") => {
            if let Some(&mode) = parts.get(1) {
                match mode.trim() {
                    "month" | "1" => state.view_mode = ViewMode::Month,
                    "week" | "2" => state.view_mode = ViewMode::Week,
                    "day" | "3" => state.view_mode = ViewMode::Day,
                    _ => {
                        state.status = format!("Unknown view: {mode}");
                        return;
                    }
                }
                refresh_cache(state);
                state.status.clear();
            } else {
                state.status = "Usage: :view month|week|day".to_string();
            }
        }
        Some("goto") => {
            if let Some(&date_str) = parts.get(1) {
                match NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d") {
                    Ok(date) => {
                        state.cursor_date = date;
                        state.year = date.year();
                        state.month = date.month();
                        refresh_cache(state);
                        state.status.clear();
                    }
                    Err(e) => {
                        state.status = format!("Invalid date: {e}");
                    }
                }
            } else {
                state.status = "Usage: :goto YYYY-MM-DD".to_string();
            }
        }
        Some("search") => {
            if let Some(&query) = parts.get(1) {
                let query = query.trim().to_lowercase();
                match state.store.list_all() {
                    Ok(events) => {
                        let matches: Vec<_> = events
                            .iter()
                            .filter(|e| e.title.to_lowercase().contains(&query))
                            .collect();
                        if matches.is_empty() {
                            state.status = format!("No events matching '{query}'");
                        } else {
                            // Jump to the first match
                            let first = &matches[0];
                            state.cursor_date = first.start.date();
                            state.year = first.start.date().year();
                            state.month = first.start.date().month();
                            refresh_cache(state);
                            state.status = format!(
                                "Found {} event(s) matching '{query}'",
                                matches.len()
                            );
                        }
                    }
                    Err(e) => {
                        state.status = format!("Search failed: {e}");
                    }
                }
            } else {
                state.status = "Usage: :search <query>".to_string();
            }
        }
        Some("q") | Some("quit") => {
            // The actual quit is handled by the event loop detecting exit
            state.status = "Use 'q' key to quit".to_string();
        }
        Some(other) => {
            state.status = format!("Unknown command: {other}");
        }
        None => {
            state.status.clear();
        }
    }
}
