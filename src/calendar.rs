//! Calendar date math and view computation.
//!
//! Provides month/week/day grid computation, date navigation,
//! and helper functions for calendar rendering.

use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};

/// Which day of the week the calendar starts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekStart {
    Monday,
    Sunday,
}

impl WeekStart {
    /// Parse from config string.
    #[must_use]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sunday" => Self::Sunday,
            _ => Self::Monday,
        }
    }

    /// Returns the chrono Weekday for the first day.
    #[must_use]
    pub fn weekday(self) -> Weekday {
        match self {
            Self::Monday => Weekday::Mon,
            Self::Sunday => Weekday::Sun,
        }
    }
}

/// The current calendar view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Month,
    Week,
    Day,
}

impl ViewMode {
    /// Cycle to the next view mode.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Month => Self::Week,
            Self::Week => Self::Day,
            Self::Day => Self::Month,
        }
    }
}

/// A cell in the month grid.
#[derive(Debug, Clone)]
pub struct DayCell {
    /// The date this cell represents.
    pub date: NaiveDate,
    /// Whether this date is in the currently displayed month.
    pub in_current_month: bool,
    /// Whether this date is today.
    pub is_today: bool,
}

/// Compute the month grid for a given year/month.
///
/// Returns a vector of weeks, each containing 7 `DayCell`s.
/// The grid always starts on the configured week start day.
#[must_use]
pub fn month_grid(year: i32, month: u32, week_start: WeekStart) -> Vec<Vec<DayCell>> {
    let today = Local::now().date_naive();
    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let last_of_month = last_day_of_month(year, month);

    // Find the start of the grid (the week-start day on or before the 1st)
    let mut grid_start = first_of_month;
    while grid_start.weekday() != week_start.weekday() {
        grid_start -= Duration::days(1);
    }

    let mut weeks = Vec::new();
    let mut current = grid_start;

    // Generate 6 weeks (covers all possible month layouts)
    for _ in 0..6 {
        let mut week = Vec::with_capacity(7);
        for _ in 0..7 {
            week.push(DayCell {
                date: current,
                in_current_month: current.month() == month && current.year() == year,
                is_today: current == today,
            });
            current += Duration::days(1);
        }
        weeks.push(week);

        // Stop if we've passed the last day and completed the week
        if current > last_of_month && current.weekday() == week_start.weekday() {
            break;
        }
    }

    weeks
}

/// Get the last day of a given month.
#[must_use]
pub fn last_day_of_month(year: i32, month: u32) -> NaiveDate {
    if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap() - Duration::days(1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap() - Duration::days(1)
    }
}

/// Get the week dates (7 days) containing the given date.
#[must_use]
pub fn week_dates(date: NaiveDate, week_start: WeekStart) -> Vec<NaiveDate> {
    let mut start = date;
    while start.weekday() != week_start.weekday() {
        start -= Duration::days(1);
    }
    (0..7).map(|i| start + Duration::days(i)).collect()
}

/// Column headers for the week.
#[must_use]
pub fn weekday_headers(week_start: WeekStart) -> Vec<&'static str> {
    let days = match week_start {
        WeekStart::Monday => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
        WeekStart::Sunday => ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    };
    days.to_vec()
}

/// Navigate to the previous month.
#[must_use]
pub fn prev_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

/// Navigate to the next month.
#[must_use]
pub fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

/// Month name from number.
#[must_use]
pub fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Unknown",
    }
}

/// Hours for time slot rendering (0..24).
#[must_use]
pub fn hour_labels(use_24h: bool) -> Vec<String> {
    (0..24)
        .map(|h| {
            if use_24h {
                format!("{h:02}:00")
            } else {
                let period = if h < 12 { "AM" } else { "PM" };
                let h12 = if h == 0 {
                    12
                } else if h > 12 {
                    h - 12
                } else {
                    h
                };
                format!("{h12}:00 {period}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_grid_has_correct_structure() {
        let grid = month_grid(2026, 3, WeekStart::Monday);
        assert!(!grid.is_empty());
        for week in &grid {
            assert_eq!(week.len(), 7);
        }
    }

    #[test]
    fn month_grid_starts_on_correct_day() {
        let grid = month_grid(2026, 3, WeekStart::Monday);
        assert_eq!(grid[0][0].date.weekday(), Weekday::Mon);

        let grid_sun = month_grid(2026, 3, WeekStart::Sunday);
        assert_eq!(grid_sun[0][0].date.weekday(), Weekday::Sun);
    }

    #[test]
    fn month_grid_contains_first_and_last() {
        let grid = month_grid(2026, 3, WeekStart::Monday);
        let all_dates: Vec<NaiveDate> = grid.iter().flatten().map(|c| c.date).collect();
        let first = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let last = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        assert!(all_dates.contains(&first));
        assert!(all_dates.contains(&last));
    }

    #[test]
    fn last_day_of_month_february_leap() {
        let d = last_day_of_month(2024, 2);
        assert_eq!(d.day(), 29);
    }

    #[test]
    fn last_day_of_month_february_normal() {
        let d = last_day_of_month(2025, 2);
        assert_eq!(d.day(), 28);
    }

    #[test]
    fn last_day_of_month_december() {
        let d = last_day_of_month(2026, 12);
        assert_eq!(d.day(), 31);
    }

    #[test]
    fn week_dates_returns_seven() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
        let dates = week_dates(date, WeekStart::Monday);
        assert_eq!(dates.len(), 7);
        assert_eq!(dates[0].weekday(), Weekday::Mon);
    }

    #[test]
    fn prev_next_month_navigation() {
        assert_eq!(prev_month(2026, 1), (2025, 12));
        assert_eq!(prev_month(2026, 6), (2026, 5));
        assert_eq!(next_month(2026, 12), (2027, 1));
        assert_eq!(next_month(2026, 6), (2026, 7));
    }

    #[test]
    fn weekday_headers_correct_length() {
        assert_eq!(weekday_headers(WeekStart::Monday).len(), 7);
        assert_eq!(weekday_headers(WeekStart::Sunday).len(), 7);
        assert_eq!(weekday_headers(WeekStart::Monday)[0], "Mon");
        assert_eq!(weekday_headers(WeekStart::Sunday)[0], "Sun");
    }

    #[test]
    fn hour_labels_24h() {
        let labels = hour_labels(true);
        assert_eq!(labels.len(), 24);
        assert_eq!(labels[0], "00:00");
        assert_eq!(labels[23], "23:00");
    }

    #[test]
    fn hour_labels_12h() {
        let labels = hour_labels(false);
        assert_eq!(labels.len(), 24);
        assert_eq!(labels[0], "12:00 AM");
        assert_eq!(labels[12], "12:00 PM");
        assert_eq!(labels[13], "1:00 PM");
    }

    #[test]
    fn view_mode_cycle() {
        assert_eq!(ViewMode::Month.next(), ViewMode::Week);
        assert_eq!(ViewMode::Week.next(), ViewMode::Day);
        assert_eq!(ViewMode::Day.next(), ViewMode::Month);
    }
}
