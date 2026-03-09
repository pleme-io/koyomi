//! Rhai scripting plugin system.
//!
//! Loads user scripts from `~/.config/koyomi/scripts/*.rhai` and registers
//! app-specific functions for calendar automation.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use soushi::ScriptEngine;

/// Event hooks that scripts can define.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEvent {
    /// Fired when the app starts.
    OnStart,
    /// Fired when the app is quitting.
    OnQuit,
    /// Fired on key press with the key name.
    OnKey(String),
}

/// Manages the Rhai scripting engine with koyomi-specific functions.
pub struct KoyomiScriptEngine {
    engine: ScriptEngine,
    /// Shared state for script-triggered actions.
    pub pending_actions: Arc<Mutex<Vec<ScriptAction>>>,
}

/// Actions that scripts can trigger.
#[derive(Debug, Clone)]
pub enum ScriptAction {
    /// Add a calendar event.
    AddEvent { title: String, date: String },
    /// Navigate to a specific date.
    NavigateDate(String),
}

impl KoyomiScriptEngine {
    /// Create a new scripting engine with koyomi-specific functions registered.
    #[must_use]
    pub fn new() -> Self {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_log();
        engine.register_builtin_env();
        engine.register_builtin_string();

        let pending = Arc::new(Mutex::new(Vec::<ScriptAction>::new()));

        // Register koyomi.add_event(title, date)
        let p = Arc::clone(&pending);
        engine.register_fn("koyomi_add_event", move |title: &str, date: &str| {
            if let Ok(mut actions) = p.lock() {
                actions.push(ScriptAction::AddEvent {
                    title: title.to_string(),
                    date: date.to_string(),
                });
            }
        });

        // Register koyomi.list_events(date) — returns empty array (placeholder)
        engine.register_fn("koyomi_list_events", |_date: &str| -> soushi::rhai::Array {
            soushi::rhai::Array::new()
        });

        // Register koyomi.today() — returns today's date as YYYY-MM-DD
        engine.register_fn("koyomi_today", || -> String {
            chrono::Local::now().format("%Y-%m-%d").to_string()
        });

        // Register koyomi.navigate(date)
        let p = Arc::clone(&pending);
        engine.register_fn("koyomi_navigate", move |date: &str| {
            if let Ok(mut actions) = p.lock() {
                actions.push(ScriptAction::NavigateDate(date.to_string()));
            }
        });

        Self {
            engine,
            pending_actions: pending,
        }
    }

    /// Load scripts from the default config directory.
    pub fn load_user_scripts(&mut self) {
        let scripts_dir = scripts_dir();
        if scripts_dir.is_dir() {
            match self.engine.load_scripts_dir(&scripts_dir) {
                Ok(names) => {
                    if !names.is_empty() {
                        tracing::info!(count = names.len(), "loaded koyomi scripts: {names:?}");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to load koyomi scripts");
                }
            }
        }
    }

    /// Fire an event hook.
    pub fn fire_event(&self, event: &ScriptEvent) {
        let hook_name = match event {
            ScriptEvent::OnStart => "on_start",
            ScriptEvent::OnQuit => "on_quit",
            ScriptEvent::OnKey(_) => "on_key",
        };

        let script = match event {
            ScriptEvent::OnKey(key) => format!("if is_def_fn(\"{hook_name}\", 1) {{ {hook_name}(\"{key}\"); }}"),
            _ => format!("if is_def_fn(\"{hook_name}\", 0) {{ {hook_name}(); }}"),
        };

        if let Err(e) = self.engine.eval(&script) {
            tracing::debug!(hook = hook_name, error = %e, "script hook not defined or failed");
        }
    }

    /// Drain any pending actions triggered by scripts.
    pub fn drain_actions(&self) -> Vec<ScriptAction> {
        if let Ok(mut actions) = self.pending_actions.lock() {
            actions.drain(..).collect()
        } else {
            Vec::new()
        }
    }
}

impl Default for KoyomiScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Default scripts directory: `~/.config/koyomi/scripts/`.
fn scripts_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("koyomi")
        .join("scripts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_creation() {
        let _engine = KoyomiScriptEngine::new();
    }

    #[test]
    fn add_event_action() {
        let engine = KoyomiScriptEngine::new();
        engine
            .engine
            .eval(r#"koyomi_add_event("Meeting", "2026-03-15")"#)
            .unwrap();
        let actions = engine.drain_actions();
        assert_eq!(actions.len(), 1);
        assert!(
            matches!(&actions[0], ScriptAction::AddEvent { title, date } if title == "Meeting" && date == "2026-03-15")
        );
    }

    #[test]
    fn list_events_returns_array() {
        let engine = KoyomiScriptEngine::new();
        let result = engine
            .engine
            .eval(r#"koyomi_list_events("2026-03-15")"#)
            .unwrap();
        assert!(result.is_array());
    }

    #[test]
    fn today_returns_date_string() {
        let engine = KoyomiScriptEngine::new();
        let result = engine.engine.eval("koyomi_today()").unwrap();
        let s = result.into_string().unwrap();
        // Should be YYYY-MM-DD format
        assert_eq!(s.len(), 10);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
    }

    #[test]
    fn navigate_date_action() {
        let engine = KoyomiScriptEngine::new();
        engine
            .engine
            .eval(r#"koyomi_navigate("2026-01-01")"#)
            .unwrap();
        let actions = engine.drain_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ScriptAction::NavigateDate(d) if d == "2026-01-01"));
    }

    #[test]
    fn fire_event_does_not_panic() {
        let engine = KoyomiScriptEngine::new();
        engine.fire_event(&ScriptEvent::OnStart);
        engine.fire_event(&ScriptEvent::OnQuit);
        engine.fire_event(&ScriptEvent::OnKey("j".to_string()));
    }

    #[test]
    fn drain_actions_clears() {
        let engine = KoyomiScriptEngine::new();
        engine
            .engine
            .eval(r#"koyomi_navigate("2026-06-01")"#)
            .unwrap();
        assert_eq!(engine.drain_actions().len(), 1);
        assert!(engine.drain_actions().is_empty());
    }
}
