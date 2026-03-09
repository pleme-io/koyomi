//! Reminder scheduling and notification dispatch.
//!
//! Checks upcoming events for reminders and prints notifications
//! when the reminder time arrives. Designed to run in the background
//! (daemon mode) or as a periodic check during GUI operation.

use chrono::{Duration, Local, NaiveDateTime};
use std::collections::HashSet;

use crate::events::{EventOccurrence, EventStore, EventStoreError};

/// Tracks which reminders have already been fired to avoid duplicates.
pub struct ReminderScheduler {
    /// Set of (event_id, occurrence_start, reminder_minutes) that have been fired.
    fired: HashSet<(String, NaiveDateTime, i32)>,
}

impl ReminderScheduler {
    #[must_use]
    pub fn new() -> Self {
        Self {
            fired: HashSet::new(),
        }
    }

    /// Check for reminders that should fire now.
    ///
    /// Returns a list of reminder messages for events whose reminder
    /// window has arrived but hasn't been fired yet.
    pub fn check(
        &mut self,
        store: &EventStore,
    ) -> Result<Vec<ReminderNotification>, EventStoreError> {
        let now = Local::now().naive_local();
        // Look ahead 24 hours for upcoming reminders
        let look_ahead = now + Duration::hours(24);
        let occurrences = store.query_range(now, look_ahead)?;

        let mut notifications = Vec::new();

        for occ in &occurrences {
            for &reminder_mins in &occ.event.reminders {
                let reminder_time =
                    occ.occurrence_start - Duration::minutes(i64::from(reminder_mins));
                let key = (
                    occ.event.id.clone(),
                    occ.occurrence_start,
                    reminder_mins,
                );

                // Fire if we're past the reminder time and haven't fired it yet
                if now >= reminder_time && !self.fired.contains(&key) {
                    self.fired.insert(key);
                    notifications.push(ReminderNotification {
                        event_title: occ.event.title.clone(),
                        event_start: occ.occurrence_start,
                        minutes_before: reminder_mins,
                        event_id: occ.event.id.clone(),
                    });
                }
            }
        }

        Ok(notifications)
    }

    /// Clear all fired reminders (e.g., at start of new day).
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.fired.clear();
    }

    /// Number of fired reminders tracked.
    #[must_use]
    #[allow(dead_code)]
    pub fn fired_count(&self) -> usize {
        self.fired.len()
    }
}

impl Default for ReminderScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// A notification that a reminder has fired.
#[derive(Debug, Clone)]
pub struct ReminderNotification {
    /// The event title.
    pub event_title: String,
    /// The event start time.
    pub event_start: NaiveDateTime,
    /// How many minutes before the event this reminder was set.
    pub minutes_before: i32,
    /// The event ID.
    #[allow(dead_code)]
    pub event_id: String,
}

impl std::fmt::Display for ReminderNotification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Reminder: \"{}\" starts at {} ({} min)",
            self.event_title,
            self.event_start.format("%H:%M"),
            self.minutes_before,
        )
    }
}

/// Get all upcoming reminders as a summary for the GUI status line.
#[must_use]
#[allow(dead_code)]
pub fn upcoming_reminders(occurrences: &[EventOccurrence]) -> Vec<String> {
    let now = Local::now().naive_local();
    let mut reminders = Vec::new();

    for occ in occurrences {
        for &mins in &occ.event.reminders {
            let reminder_time = occ.occurrence_start - Duration::minutes(i64::from(mins));
            if reminder_time > now {
                let until = reminder_time.signed_duration_since(now);
                let until_mins = until.num_minutes();
                if until_mins <= 60 {
                    reminders.push(format!(
                        "{} in {} min",
                        occ.event.title, until_mins
                    ));
                }
            }
        }
    }

    reminders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_scheduler_starts_empty() {
        let sched = ReminderScheduler::new();
        assert_eq!(sched.fired_count(), 0);
    }

    #[test]
    fn reminder_scheduler_clear() {
        let mut sched = ReminderScheduler::new();
        sched.fired.insert(("test".to_string(), Local::now().naive_local(), 15));
        assert_eq!(sched.fired_count(), 1);
        sched.clear();
        assert_eq!(sched.fired_count(), 0);
    }

    #[test]
    fn reminder_notification_display() {
        use chrono::NaiveDate;
        let notif = ReminderNotification {
            event_title: "Meeting".to_string(),
            event_start: NaiveDate::from_ymd_opt(2026, 3, 10)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            minutes_before: 15,
            event_id: "test-id".to_string(),
        };
        let s = format!("{notif}");
        assert!(s.contains("Meeting"));
        assert!(s.contains("10:00"));
        assert!(s.contains("15 min"));
    }
}
