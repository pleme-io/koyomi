//! macOS calendar backend using EventKit / CalDAV.

use crate::platform::{CalendarBackend, CalendarEvent, DateTime};

/// macOS calendar backend.
pub struct MacOSCalendarBackend;

impl MacOSCalendarBackend {
    pub fn new() -> Self {
        Self
    }
}

impl CalendarBackend for MacOSCalendarBackend {
    fn list_events(
        &self,
        _start: DateTime,
        _end: DateTime,
    ) -> Result<Vec<CalendarEvent>, Box<dyn std::error::Error>> {
        // TODO: implement via EventKit or CalDAV
        tracing::warn!("event listing not yet implemented");
        Ok(Vec::new())
    }

    fn create_event(
        &self,
        _event: &CalendarEvent,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // TODO: implement via EventKit or CalDAV
        tracing::warn!("event creation not yet implemented");
        Ok(String::new())
    }

    fn update_event(
        &self,
        _id: &str,
        _event: &CalendarEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: implement via EventKit or CalDAV
        tracing::warn!("event update not yet implemented");
        Ok(())
    }

    fn delete_event(&self, _id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: implement via EventKit or CalDAV
        tracing::warn!("event deletion not yet implemented");
        Ok(())
    }
}
