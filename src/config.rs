//! Koyomi configuration — uses shikumi for discovery and hot-reload.

use serde::{Deserialize, Serialize};

/// Top-level configuration.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct KoyomiConfig {
    pub appearance: AppearanceConfig,
    pub calendars: Vec<CalendarSource>,
    pub notifications: NotificationConfig,
    pub sync: SyncConfig,
    pub daemon: DaemonConfig,
}

impl Default for KoyomiConfig {
    fn default() -> Self {
        Self {
            appearance: AppearanceConfig::default(),
            calendars: Vec::new(),
            notifications: NotificationConfig::default(),
            sync: SyncConfig::default(),
            daemon: DaemonConfig::default(),
        }
    }
}

/// Visual appearance settings.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct AppearanceConfig {
    /// Window width in pixels.
    pub width: u32,
    /// Window height in pixels.
    pub height: u32,
    /// Font size in points.
    pub font_size: f32,
    /// Window opacity (0.0-1.0).
    pub opacity: f32,
    /// First day of the week: "monday" or "sunday".
    pub week_start: String,
    /// Time format: "12h" or "24h".
    pub time_format: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            width: 900,
            height: 700,
            font_size: 14.0,
            opacity: 1.0,
            week_start: "monday".into(),
            time_format: "24h".into(),
        }
    }
}

/// A CalDAV calendar source.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct CalendarSource {
    /// Human-readable calendar name.
    pub name: String,
    /// CalDAV URL.
    pub url: String,
    /// Display color (hex string).
    pub color: Option<String>,
    /// Whether this calendar is enabled.
    pub enabled: bool,
}

/// Notification/reminder settings.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct NotificationConfig {
    /// Enable notifications.
    pub enabled: bool,
    /// Default reminder in minutes before event.
    pub default_reminder_mins: i32,
    /// Enable notification sound.
    pub sound: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_reminder_mins: 15,
            sound: true,
        }
    }
}

/// Calendar sync settings.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct SyncConfig {
    /// Sync interval in seconds.
    pub interval_secs: u32,
    /// Enable offline mode (cache events locally).
    pub offline_mode: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            interval_secs: 300,
            offline_mode: false,
        }
    }
}

/// Daemon mode configuration.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct DaemonConfig {
    /// Enable daemon mode.
    pub enable: bool,
    /// Listen address for the daemon.
    pub listen_addr: String,
    /// Database URL for event storage.
    pub database_url: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            enable: false,
            listen_addr: "0.0.0.0:50052".into(),
            database_url: "sqlite:///tmp/kodate/state.db".into(),
        }
    }
}
