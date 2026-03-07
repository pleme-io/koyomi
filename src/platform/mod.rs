//! Platform abstraction traits for calendar backends.
//!
//! Each platform or backend provides a `CalendarBackend` implementation
//! for managing calendar events via CalDAV or native APIs.

#[cfg(target_os = "macos")]
pub mod macos;

/// A calendar event.
#[derive(Debug, Clone)]
pub struct CalendarEvent {
    /// Unique event identifier.
    pub id: String,
    /// Event title.
    pub title: String,
    /// Event description.
    pub description: Option<String>,
    /// Event start time.
    pub start: DateTime,
    /// Event end time.
    pub end: DateTime,
    /// Event location.
    pub location: Option<String>,
    /// Calendar this event belongs to.
    pub calendar: String,
    /// Reminder offsets in minutes before the event.
    pub reminders: Vec<i32>,
}

/// Date-time type alias.
pub type DateTime = chrono::NaiveDateTime;

/// Calendar backend for reading and writing events.
pub trait CalendarBackend: Send + Sync {
    /// List events within a time range.
    fn list_events(
        &self,
        start: DateTime,
        end: DateTime,
    ) -> Result<Vec<CalendarEvent>, Box<dyn std::error::Error>>;

    /// Create a new event, returning its ID.
    fn create_event(
        &self,
        event: &CalendarEvent,
    ) -> Result<String, Box<dyn std::error::Error>>;

    /// Update an existing event by ID.
    fn update_event(
        &self,
        id: &str,
        event: &CalendarEvent,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Delete an event by ID.
    fn delete_event(&self, id: &str) -> Result<(), Box<dyn std::error::Error>>;
}

/// Create a platform-specific calendar backend.
pub fn create_backend() -> Box<dyn CalendarBackend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOSCalendarBackend::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        panic!("calendar backend not implemented for this platform")
    }
}
