//! Recurrence rule parsing and occurrence expansion.
//!
//! Supports RRULE-style recurrence: daily, weekly, monthly, yearly
//! with optional interval, count limit, and until date.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// Recurrence frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// A recurrence rule defining how an event repeats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurrenceRule {
    /// How often the event repeats.
    pub freq: Frequency,
    /// Repeat every N intervals (default 1).
    #[serde(default = "default_interval")]
    pub interval: u32,
    /// Maximum number of occurrences (None = infinite).
    pub count: Option<u32>,
    /// Repeat until this date (exclusive).
    pub until: Option<NaiveDate>,
    /// For weekly: which days of the week (0=Mon, 6=Sun).
    #[serde(default)]
    pub by_weekday: Vec<u8>,
}

fn default_interval() -> u32 {
    1
}

impl Default for RecurrenceRule {
    fn default() -> Self {
        Self {
            freq: Frequency::Daily,
            interval: 1,
            count: None,
            until: None,
            by_weekday: Vec::new(),
        }
    }
}

impl RecurrenceRule {
    /// Expand occurrences of a recurring event within the given date range.
    ///
    /// `event_start` is the original event start time. Returns all occurrence
    /// start times that fall within `[range_start, range_end)`.
    #[must_use]
    pub fn occurrences(
        &self,
        event_start: NaiveDateTime,
        range_start: NaiveDateTime,
        range_end: NaiveDateTime,
    ) -> Vec<NaiveDateTime> {
        let mut results = Vec::new();
        let mut current = event_start;
        let mut count = 0u32;
        let max_iterations = 10_000; // safety limit
        let mut iterations = 0;

        loop {
            if iterations >= max_iterations {
                break;
            }
            iterations += 1;

            // Check count limit
            if let Some(max_count) = self.count {
                if count >= max_count {
                    break;
                }
            }

            // Check until limit
            if let Some(until) = self.until {
                if current.date() > until {
                    break;
                }
            }

            // Past the query range entirely
            if current >= range_end {
                break;
            }

            // For weekly with by_weekday, check if the current day matches
            let day_matches = if self.freq == Frequency::Weekly && !self.by_weekday.is_empty() {
                let wd = current.weekday().num_days_from_monday() as u8;
                self.by_weekday.contains(&wd)
            } else {
                true
            };

            if day_matches && current >= range_start {
                results.push(current);
            }

            if day_matches {
                count += 1;
            }

            // Advance to next occurrence
            current = self.advance(current);
        }

        results
    }

    /// Advance a datetime to the next occurrence.
    fn advance(&self, dt: NaiveDateTime) -> NaiveDateTime {
        match self.freq {
            Frequency::Daily => dt + Duration::days(i64::from(self.interval)),
            Frequency::Weekly => {
                if self.by_weekday.is_empty() {
                    dt + Duration::weeks(i64::from(self.interval))
                } else {
                    // Advance one day at a time for by_weekday matching
                    dt + Duration::days(1)
                }
            }
            Frequency::Monthly => {
                let date = dt.date();
                let day = date.day();
                let mut month = date.month() + self.interval;
                let mut year = date.year();
                while month > 12 {
                    month -= 12;
                    year += 1;
                }
                // Clamp day to valid range for target month
                let max_day = last_day_of(year, month);
                let clamped_day = day.min(max_day);
                let new_date = NaiveDate::from_ymd_opt(year, month, clamped_day).unwrap();
                new_date.and_time(dt.time())
            }
            Frequency::Yearly => {
                let date = dt.date();
                let new_year = date.year() + self.interval as i32;
                let month = date.month();
                let day = date.day().min(last_day_of(new_year, month));
                let new_date = NaiveDate::from_ymd_opt(new_year, month, day).unwrap();
                new_date.and_time(dt.time())
            }
        }
    }
}

/// Get the last day number of a given month.
fn last_day_of(year: i32, month: u32) -> u32 {
    if month == 12 {
        31
    } else {
        let next_month_first = NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap();
        (next_month_first - Duration::days(1)).day()
    }
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
    fn daily_recurrence() {
        let rule = RecurrenceRule {
            freq: Frequency::Daily,
            interval: 1,
            count: Some(5),
            ..Default::default()
        };
        let start = dt(2026, 3, 1, 9, 0);
        let range_start = dt(2026, 3, 1, 0, 0);
        let range_end = dt(2026, 3, 31, 23, 59);
        let occ = rule.occurrences(start, range_start, range_end);
        assert_eq!(occ.len(), 5);
        assert_eq!(occ[0], dt(2026, 3, 1, 9, 0));
        assert_eq!(occ[4], dt(2026, 3, 5, 9, 0));
    }

    #[test]
    fn daily_every_other_day() {
        let rule = RecurrenceRule {
            freq: Frequency::Daily,
            interval: 2,
            count: Some(3),
            ..Default::default()
        };
        let start = dt(2026, 3, 1, 10, 0);
        let range_start = dt(2026, 3, 1, 0, 0);
        let range_end = dt(2026, 3, 31, 23, 59);
        let occ = rule.occurrences(start, range_start, range_end);
        assert_eq!(occ.len(), 3);
        assert_eq!(occ[1], dt(2026, 3, 3, 10, 0));
        assert_eq!(occ[2], dt(2026, 3, 5, 10, 0));
    }

    #[test]
    fn weekly_recurrence() {
        let rule = RecurrenceRule {
            freq: Frequency::Weekly,
            interval: 1,
            count: Some(4),
            ..Default::default()
        };
        let start = dt(2026, 3, 2, 14, 0); // Monday
        let range_start = dt(2026, 3, 1, 0, 0);
        let range_end = dt(2026, 4, 30, 23, 59);
        let occ = rule.occurrences(start, range_start, range_end);
        assert_eq!(occ.len(), 4);
        assert_eq!(occ[1], dt(2026, 3, 9, 14, 0));
    }

    #[test]
    fn weekly_by_weekday() {
        let rule = RecurrenceRule {
            freq: Frequency::Weekly,
            interval: 1,
            count: Some(6),
            by_weekday: vec![0, 2, 4], // Mon, Wed, Fri
            ..Default::default()
        };
        let start = dt(2026, 3, 2, 9, 0); // Monday
        let range_start = dt(2026, 3, 1, 0, 0);
        let range_end = dt(2026, 3, 31, 23, 59);
        let occ = rule.occurrences(start, range_start, range_end);
        assert_eq!(occ.len(), 6);
        // Should be Mon, Wed, Fri, Mon, Wed, Fri
        assert_eq!(occ[0].date().weekday(), chrono::Weekday::Mon);
        assert_eq!(occ[1].date().weekday(), chrono::Weekday::Wed);
        assert_eq!(occ[2].date().weekday(), chrono::Weekday::Fri);
    }

    #[test]
    fn monthly_recurrence() {
        let rule = RecurrenceRule {
            freq: Frequency::Monthly,
            interval: 1,
            count: Some(3),
            ..Default::default()
        };
        let start = dt(2026, 1, 31, 10, 0);
        let range_start = dt(2026, 1, 1, 0, 0);
        let range_end = dt(2026, 12, 31, 23, 59);
        let occ = rule.occurrences(start, range_start, range_end);
        assert_eq!(occ.len(), 3);
        // Jan 31 -> Feb 28 (clamped) -> Mar 28 (clamped from 31 to prev result's advance)
        assert_eq!(occ[0], dt(2026, 1, 31, 10, 0));
        assert_eq!(occ[1].date().month(), 2);
        assert_eq!(occ[1].date().day(), 28); // Feb doesn't have 31
    }

    #[test]
    fn yearly_recurrence() {
        let rule = RecurrenceRule {
            freq: Frequency::Yearly,
            interval: 1,
            count: Some(3),
            ..Default::default()
        };
        let start = dt(2024, 2, 29, 12, 0); // leap day
        let range_start = dt(2024, 1, 1, 0, 0);
        let range_end = dt(2030, 12, 31, 23, 59);
        let occ = rule.occurrences(start, range_start, range_end);
        assert_eq!(occ.len(), 3);
        assert_eq!(occ[0], dt(2024, 2, 29, 12, 0));
        assert_eq!(occ[1].date().day(), 28); // 2025 is not a leap year
    }

    #[test]
    fn until_limit() {
        let rule = RecurrenceRule {
            freq: Frequency::Daily,
            interval: 1,
            count: None,
            until: Some(NaiveDate::from_ymd_opt(2026, 3, 5).unwrap()),
            ..Default::default()
        };
        let start = dt(2026, 3, 1, 9, 0);
        let range_start = dt(2026, 3, 1, 0, 0);
        let range_end = dt(2026, 3, 31, 23, 59);
        let occ = rule.occurrences(start, range_start, range_end);
        assert_eq!(occ.len(), 5);
    }

    #[test]
    fn range_filtering() {
        let rule = RecurrenceRule {
            freq: Frequency::Daily,
            interval: 1,
            count: Some(10),
            ..Default::default()
        };
        let start = dt(2026, 3, 1, 9, 0);
        // Only query March 5-7
        let range_start = dt(2026, 3, 5, 0, 0);
        let range_end = dt(2026, 3, 8, 0, 0);
        let occ = rule.occurrences(start, range_start, range_end);
        assert_eq!(occ.len(), 3);
        assert_eq!(occ[0], dt(2026, 3, 5, 9, 0));
    }
}
