//! Event data model and local YAML-based storage.
//!
//! Events are stored as individual YAML files in the data directory,
//! with CRUD operations, date range queries, and recurrence expansion.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::recurrence::RecurrenceRule;

/// A calendar event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique event identifier.
    pub id: String,
    /// Event title/summary.
    pub title: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Start date-time.
    pub start: NaiveDateTime,
    /// End date-time.
    pub end: NaiveDateTime,
    /// Optional location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Calendar name this event belongs to.
    #[serde(default = "default_calendar")]
    pub calendar: String,
    /// Display color (hex string like "#88c0d0").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Reminder offsets in minutes before the event.
    #[serde(default)]
    pub reminders: Vec<i32>,
    /// Optional recurrence rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<RecurrenceRule>,
}

fn default_calendar() -> String {
    "default".to_string()
}

impl Event {
    /// Create a new event with a generated UUID.
    #[must_use]
    pub fn new(title: String, start: NaiveDateTime, end: NaiveDateTime) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            description: None,
            start,
            end,
            location: None,
            calendar: "default".to_string(),
            color: None,
            reminders: Vec::new(),
            recurrence: None,
        }
    }

    /// Duration of the event.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.end.signed_duration_since(self.start)
    }

    /// Check if this event (or any recurrence) overlaps with the given date.
    #[must_use]
    #[allow(dead_code)]
    pub fn occurs_on(&self, date: NaiveDate) -> bool {
        let day_start = date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let day_end = date.and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap());

        if let Some(ref rule) = self.recurrence {
            let occurrences = rule.occurrences(self.start, day_start, day_end + Duration::seconds(1));
            !occurrences.is_empty()
        } else {
            // Non-recurring: check if event overlaps with the day
            self.start <= day_end && self.end >= day_start
        }
    }

    /// Get all occurrence start times within a date range.
    /// For non-recurring events, returns the single start time if it falls in range.
    #[must_use]
    pub fn occurrences_in_range(
        &self,
        range_start: NaiveDateTime,
        range_end: NaiveDateTime,
    ) -> Vec<NaiveDateTime> {
        if let Some(ref rule) = self.recurrence {
            rule.occurrences(self.start, range_start, range_end)
        } else if self.start < range_end && self.end > range_start {
            vec![self.start]
        } else {
            Vec::new()
        }
    }
}

/// An expanded event occurrence (for rendering).
/// This represents a single occurrence of a potentially recurring event.
#[derive(Debug, Clone)]
pub struct EventOccurrence {
    /// The original event.
    pub event: Event,
    /// The start time of this specific occurrence.
    pub occurrence_start: NaiveDateTime,
    /// The end time of this specific occurrence.
    pub occurrence_end: NaiveDateTime,
}

/// Local YAML-based event storage.
///
/// Events are stored as individual YAML files in a data directory.
/// File naming: `{event_id}.yaml`
pub struct EventStore {
    data_dir: PathBuf,
}

impl EventStore {
    /// Open or create an event store at the given directory.
    pub fn open(data_dir: &Path) -> Result<Self, EventStoreError> {
        fs::create_dir_all(data_dir).map_err(|e| EventStoreError::Io(e.to_string()))?;
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
        })
    }

    /// Open the default event store location.
    pub fn open_default() -> Result<Self, EventStoreError> {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("koyomi")
            .join("events");
        Self::open(&data_dir)
    }

    /// Create a new event and persist it.
    pub fn create(&self, event: &Event) -> Result<(), EventStoreError> {
        let path = self.event_path(&event.id);
        let yaml =
            serde_yaml::to_string(event).map_err(|e| EventStoreError::Serialize(e.to_string()))?;
        fs::write(&path, yaml).map_err(|e| EventStoreError::Io(e.to_string()))?;
        Ok(())
    }

    /// Read an event by ID.
    pub fn read(&self, id: &str) -> Result<Event, EventStoreError> {
        let path = self.event_path(id);
        let contents =
            fs::read_to_string(&path).map_err(|e| EventStoreError::Io(e.to_string()))?;
        let event: Event = serde_yaml::from_str(&contents)
            .map_err(|e| EventStoreError::Deserialize(e.to_string()))?;
        Ok(event)
    }

    /// Update an existing event.
    pub fn update(&self, event: &Event) -> Result<(), EventStoreError> {
        let path = self.event_path(&event.id);
        if !path.exists() {
            return Err(EventStoreError::NotFound(event.id.clone()));
        }
        let yaml =
            serde_yaml::to_string(event).map_err(|e| EventStoreError::Serialize(e.to_string()))?;
        fs::write(&path, yaml).map_err(|e| EventStoreError::Io(e.to_string()))?;
        Ok(())
    }

    /// Delete an event by ID.
    pub fn delete(&self, id: &str) -> Result<(), EventStoreError> {
        let path = self.event_path(id);
        if !path.exists() {
            return Err(EventStoreError::NotFound(id.to_string()));
        }
        fs::remove_file(&path).map_err(|e| EventStoreError::Io(e.to_string()))?;
        Ok(())
    }

    /// List all stored events.
    pub fn list_all(&self) -> Result<Vec<Event>, EventStoreError> {
        let mut events = Vec::new();
        let entries =
            fs::read_dir(&self.data_dir).map_err(|e| EventStoreError::Io(e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| EventStoreError::Io(e.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                let contents =
                    fs::read_to_string(&path).map_err(|e| EventStoreError::Io(e.to_string()))?;
                match serde_yaml::from_str::<Event>(&contents) {
                    Ok(event) => events.push(event),
                    Err(e) => {
                        tracing::warn!("skipping invalid event file {:?}: {e}", path);
                    }
                }
            }
        }
        events.sort_by_key(|e| e.start);
        Ok(events)
    }

    /// Query events that occur within a date range, expanding recurrences.
    pub fn query_range(
        &self,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Result<Vec<EventOccurrence>, EventStoreError> {
        let all_events = self.list_all()?;
        let mut occurrences = Vec::new();

        for event in &all_events {
            let occurrence_starts = event.occurrences_in_range(start, end);
            let duration = event.duration();
            for occ_start in occurrence_starts {
                occurrences.push(EventOccurrence {
                    event: event.clone(),
                    occurrence_start: occ_start,
                    occurrence_end: occ_start + duration,
                });
            }
        }

        occurrences.sort_by_key(|o| o.occurrence_start);
        Ok(occurrences)
    }

    /// Query events for a specific date.
    pub fn query_date(
        &self,
        date: NaiveDate,
    ) -> Result<Vec<EventOccurrence>, EventStoreError> {
        let start = date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let end = date.and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
            + Duration::seconds(1);
        self.query_range(start, end)
    }

    /// Get events for today.
    #[allow(dead_code)]
    pub fn today(&self) -> Result<Vec<EventOccurrence>, EventStoreError> {
        self.query_date(Local::now().date_naive())
    }

    /// Get the next upcoming event.
    #[allow(dead_code)]
    pub fn next_event(&self) -> Result<Option<EventOccurrence>, EventStoreError> {
        let now = Local::now().naive_local();
        let end = now + Duration::days(365); // look ahead 1 year
        let occs = self.query_range(now, end)?;
        Ok(occs.into_iter().next())
    }

    /// Count events on a specific date (for month view indicators).
    #[allow(dead_code)]
    pub fn count_on_date(&self, date: NaiveDate) -> Result<usize, EventStoreError> {
        Ok(self.query_date(date)?.len())
    }

    fn event_path(&self, id: &str) -> PathBuf {
        self.data_dir.join(format!("{id}.yaml"))
    }
}

/// Errors from event store operations.
#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serialize(String),
    #[error("deserialization error: {0}")]
    Deserialize(String),
    #[error("event not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn dt(year: i32, month: u32, day: u32, hour: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(hour, min, 0).unwrap())
    }

    #[test]
    fn event_creation() {
        let event = Event::new(
            "Test meeting".to_string(),
            dt(2026, 3, 10, 10, 0),
            dt(2026, 3, 10, 11, 0),
        );
        assert!(!event.id.is_empty());
        assert_eq!(event.title, "Test meeting");
        assert_eq!(event.duration(), Duration::hours(1));
    }

    #[test]
    fn event_occurs_on_single_day() {
        let event = Event::new(
            "Meeting".to_string(),
            dt(2026, 3, 10, 10, 0),
            dt(2026, 3, 10, 11, 0),
        );
        assert!(event.occurs_on(NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()));
        assert!(!event.occurs_on(NaiveDate::from_ymd_opt(2026, 3, 11).unwrap()));
    }

    #[test]
    fn event_occurs_on_multi_day() {
        let event = Event::new(
            "Conference".to_string(),
            dt(2026, 3, 10, 9, 0),
            dt(2026, 3, 12, 17, 0),
        );
        assert!(event.occurs_on(NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()));
        assert!(event.occurs_on(NaiveDate::from_ymd_opt(2026, 3, 11).unwrap()));
        assert!(event.occurs_on(NaiveDate::from_ymd_opt(2026, 3, 12).unwrap()));
        assert!(!event.occurs_on(NaiveDate::from_ymd_opt(2026, 3, 13).unwrap()));
    }

    #[test]
    fn store_crud() {
        let dir = tempfile::tempdir().unwrap();
        let store = EventStore::open(dir.path()).unwrap();

        // Create
        let event = Event::new("Test".to_string(), dt(2026, 3, 10, 10, 0), dt(2026, 3, 10, 11, 0));
        let id = event.id.clone();
        store.create(&event).unwrap();

        // Read
        let loaded = store.read(&id).unwrap();
        assert_eq!(loaded.title, "Test");

        // Update
        let mut updated = loaded;
        updated.title = "Updated".to_string();
        store.update(&updated).unwrap();
        let reloaded = store.read(&id).unwrap();
        assert_eq!(reloaded.title, "Updated");

        // Delete
        store.delete(&id).unwrap();
        assert!(store.read(&id).is_err());
    }

    #[test]
    fn store_list_all() {
        let dir = tempfile::tempdir().unwrap();
        let store = EventStore::open(dir.path()).unwrap();

        let e1 = Event::new("First".to_string(), dt(2026, 3, 10, 10, 0), dt(2026, 3, 10, 11, 0));
        let e2 = Event::new("Second".to_string(), dt(2026, 3, 11, 14, 0), dt(2026, 3, 11, 15, 0));
        store.create(&e1).unwrap();
        store.create(&e2).unwrap();

        let all = store.list_all().unwrap();
        assert_eq!(all.len(), 2);
        // Should be sorted by start time
        assert_eq!(all[0].title, "First");
        assert_eq!(all[1].title, "Second");
    }

    #[test]
    fn store_query_range() {
        let dir = tempfile::tempdir().unwrap();
        let store = EventStore::open(dir.path()).unwrap();

        let e1 = Event::new("Morning".to_string(), dt(2026, 3, 10, 9, 0), dt(2026, 3, 10, 10, 0));
        let e2 = Event::new("Afternoon".to_string(), dt(2026, 3, 10, 14, 0), dt(2026, 3, 10, 15, 0));
        let e3 = Event::new("Tomorrow".to_string(), dt(2026, 3, 11, 10, 0), dt(2026, 3, 11, 11, 0));
        store.create(&e1).unwrap();
        store.create(&e2).unwrap();
        store.create(&e3).unwrap();

        // Query just March 10
        let occs = store.query_range(dt(2026, 3, 10, 0, 0), dt(2026, 3, 11, 0, 0)).unwrap();
        assert_eq!(occs.len(), 2);
    }

    #[test]
    fn store_recurring_query() {
        let dir = tempfile::tempdir().unwrap();
        let store = EventStore::open(dir.path()).unwrap();

        use crate::recurrence::Frequency;
        let mut event = Event::new(
            "Daily standup".to_string(),
            dt(2026, 3, 1, 9, 0),
            dt(2026, 3, 1, 9, 30),
        );
        event.recurrence = Some(RecurrenceRule {
            freq: Frequency::Daily,
            interval: 1,
            count: Some(31),
            ..Default::default()
        });
        store.create(&event).unwrap();

        // Query March 10 — should have the standup
        let occs = store.query_date(NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()).unwrap();
        assert_eq!(occs.len(), 1);
        assert_eq!(occs[0].event.title, "Daily standup");
        assert_eq!(occs[0].occurrence_start, dt(2026, 3, 10, 9, 0));
    }

    #[test]
    fn delete_nonexistent_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = EventStore::open(dir.path()).unwrap();
        assert!(store.delete("nonexistent").is_err());
    }

    #[test]
    fn event_yaml_roundtrip() {
        let event = Event::new("Test".to_string(), dt(2026, 3, 10, 10, 0), dt(2026, 3, 10, 11, 0));
        let yaml = serde_yaml::to_string(&event).unwrap();
        let loaded: Event = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(loaded.id, event.id);
        assert_eq!(loaded.title, event.title);
        assert_eq!(loaded.start, event.start);
    }
}
