//! MCP server for Koyomi calendar via kaname.
//!
//! Exposes calendar tools over the Model Context Protocol (stdio transport),
//! allowing AI assistants to query and manage calendar events.

use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use kaname::rmcp;
use kaname::ToolResponse;
use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::calendar::WeekStart;
use crate::config::KoyomiConfig;
use crate::events::{Event, EventStore};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
struct ListEventsRequest {
    /// Date to list events for (YYYY-MM-DD). Defaults to today.
    date: Option<String>,
    /// Number of days to include from the start date. Defaults to 1.
    range_days: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateEventRequest {
    /// Event title.
    title: String,
    /// Start time (ISO 8601: YYYY-MM-DDTHH:MM).
    start: String,
    /// End time (ISO 8601: YYYY-MM-DDTHH:MM).
    end: String,
    /// Calendar name. Defaults to "default".
    calendar: Option<String>,
    /// Event location.
    location: Option<String>,
    /// Event description.
    description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteEventRequest {
    /// Event ID to delete.
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetDayRequest {
    /// Date (YYYY-MM-DD). Defaults to today.
    date: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetWeekRequest {
    /// Any date in the target week (YYYY-MM-DD). Defaults to today.
    date: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SetReminderRequest {
    /// Event ID to set a reminder for.
    id: String,
    /// Reminder offset in minutes before the event.
    minutes: i32,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigGetRequest {
    /// Config key to retrieve (dot-separated path, e.g. "appearance.font_size").
    key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigSetRequest {
    /// Config key to set.
    key: String,
    /// Value to set (as a string).
    value: String,
}

// ---------------------------------------------------------------------------
// MCP Service
// ---------------------------------------------------------------------------

/// Koyomi MCP server.
pub struct KoyomiMcpServer {
    tool_router: ToolRouter<Self>,
    store: EventStore,
    config: KoyomiConfig,
}

impl std::fmt::Debug for KoyomiMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KoyomiMcpServer").finish()
    }
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("Invalid date '{s}': {e}"))
}

fn parse_datetime(s: &str) -> Result<NaiveDateTime, String> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|e| format!("Invalid datetime '{s}': {e}"))
}

#[tool_router]
impl KoyomiMcpServer {
    pub fn new(config: KoyomiConfig) -> Result<Self, String> {
        let store = EventStore::open_default().map_err(|e| format!("{e}"))?;
        Ok(Self {
            tool_router: Self::tool_router(),
            store,
            config,
        })
    }

    // -- Standard tools --

    #[tool(description = "Get Koyomi server status and statistics.")]
    async fn status(&self) -> Result<CallToolResult, McpError> {
        let event_count = self.store.list_all().map(|e| e.len()).unwrap_or(0);
        let today = Local::now().date_naive();
        let today_count = self.store.query_date(today).map(|e| e.len()).unwrap_or(0);
        Ok(ToolResponse::success(&serde_json::json!({
            "status": "running",
            "total_events": event_count,
            "events_today": today_count,
            "date": today.to_string(),
        })))
    }

    #[tool(description = "Get the Koyomi version.")]
    async fn version(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(&serde_json::json!({
            "name": "koyomi",
            "version": env!("CARGO_PKG_VERSION"),
        })))
    }

    #[tool(description = "Get a configuration value by key.")]
    async fn config_get(
        &self,
        Parameters(req): Parameters<ConfigGetRequest>,
    ) -> Result<CallToolResult, McpError> {
        let json = serde_json::to_value(&self.config).unwrap_or_default();
        let value = req
            .key
            .split('.')
            .fold(Some(&json), |v, k| v.and_then(|v| v.get(k)));
        match value {
            Some(v) => Ok(ToolResponse::success(v)),
            None => Ok(ToolResponse::error(&format!("Key '{}' not found", req.key))),
        }
    }

    #[tool(description = "Set a configuration value (runtime only, not persisted).")]
    async fn config_set(
        &self,
        Parameters(req): Parameters<ConfigSetRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Runtime config mutation is complex with shikumi; acknowledge the request.
        Ok(ToolResponse::text(&format!(
            "Config key '{}' would be set to '{}'. Runtime config mutation is not yet supported; \
             edit ~/.config/koyomi/koyomi.yaml instead.",
            req.key, req.value
        )))
    }

    // -- App-specific tools --

    #[tool(
        description = "List events for a date range. Defaults to today. Use range_days to extend."
    )]
    async fn list_events(
        &self,
        Parameters(req): Parameters<ListEventsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let date = match &req.date {
            Some(s) => parse_date(s).map_err(|e| McpError::invalid_params(e, None))?,
            None => Local::now().date_naive(),
        };
        let days = req.range_days.unwrap_or(1);
        let start = date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let end = start + Duration::days(days);

        match self.store.query_range(start, end) {
            Ok(occs) => {
                let events: Vec<serde_json::Value> = occs
                    .iter()
                    .map(|occ| {
                        serde_json::json!({
                            "id": occ.event.id,
                            "title": occ.event.title,
                            "start": occ.occurrence_start.format("%Y-%m-%dT%H:%M").to_string(),
                            "end": occ.occurrence_end.format("%Y-%m-%dT%H:%M").to_string(),
                            "calendar": occ.event.calendar,
                            "location": occ.event.location,
                        })
                    })
                    .collect();
                Ok(ToolResponse::success(&serde_json::json!({
                    "date": date.to_string(),
                    "range_days": days,
                    "count": events.len(),
                    "events": events,
                })))
            }
            Err(e) => Ok(ToolResponse::error(&format!("Query failed: {e}"))),
        }
    }

    #[tool(description = "Create a new calendar event.")]
    async fn create_event(
        &self,
        Parameters(req): Parameters<CreateEventRequest>,
    ) -> Result<CallToolResult, McpError> {
        let start = parse_datetime(&req.start).map_err(|e| McpError::invalid_params(e, None))?;
        let end = parse_datetime(&req.end).map_err(|e| McpError::invalid_params(e, None))?;

        let mut event = Event::new(req.title.clone(), start, end);
        event.calendar = req.calendar.unwrap_or_else(|| "default".to_string());
        event.location = req.location;
        event.description = req.description;
        event.reminders = vec![self.config.notifications.default_reminder_mins];

        match self.store.create(&event) {
            Ok(()) => Ok(ToolResponse::success(&serde_json::json!({
                "created": true,
                "id": event.id,
                "title": event.title,
                "start": event.start.format("%Y-%m-%dT%H:%M").to_string(),
                "end": event.end.format("%Y-%m-%dT%H:%M").to_string(),
            }))),
            Err(e) => Ok(ToolResponse::error(&format!("Create failed: {e}"))),
        }
    }

    #[tool(description = "Delete a calendar event by ID.")]
    async fn delete_event(
        &self,
        Parameters(req): Parameters<DeleteEventRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.store.delete(&req.id) {
            Ok(()) => Ok(ToolResponse::success(&serde_json::json!({
                "deleted": true,
                "id": req.id,
            }))),
            Err(e) => Ok(ToolResponse::error(&format!("Delete failed: {e}"))),
        }
    }

    #[tool(description = "Get all events for a specific day.")]
    async fn get_today(
        &self,
        Parameters(req): Parameters<GetDayRequest>,
    ) -> Result<CallToolResult, McpError> {
        let date = match &req.date {
            Some(s) => parse_date(s).map_err(|e| McpError::invalid_params(e, None))?,
            None => Local::now().date_naive(),
        };
        match self.store.query_date(date) {
            Ok(occs) => {
                let events: Vec<serde_json::Value> = occs
                    .iter()
                    .map(|occ| {
                        serde_json::json!({
                            "id": occ.event.id,
                            "title": occ.event.title,
                            "start": occ.occurrence_start.format("%H:%M").to_string(),
                            "end": occ.occurrence_end.format("%H:%M").to_string(),
                            "calendar": occ.event.calendar,
                            "location": occ.event.location,
                        })
                    })
                    .collect();
                Ok(ToolResponse::success(&serde_json::json!({
                    "date": date.to_string(),
                    "day": date.format("%A").to_string(),
                    "count": events.len(),
                    "events": events,
                })))
            }
            Err(e) => Ok(ToolResponse::error(&format!("Query failed: {e}"))),
        }
    }

    #[tool(description = "Get all events for the week containing the given date.")]
    async fn get_week(
        &self,
        Parameters(req): Parameters<GetWeekRequest>,
    ) -> Result<CallToolResult, McpError> {
        let date = match &req.date {
            Some(s) => parse_date(s).map_err(|e| McpError::invalid_params(e, None))?,
            None => Local::now().date_naive(),
        };
        let week_start = WeekStart::from_str(&self.config.appearance.week_start);
        let dates = crate::calendar::week_dates(date, week_start);
        let start = dates[0].and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let end =
            dates[6].and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap()) + Duration::seconds(1);

        match self.store.query_range(start, end) {
            Ok(occs) => {
                let events: Vec<serde_json::Value> = occs
                    .iter()
                    .map(|occ| {
                        serde_json::json!({
                            "id": occ.event.id,
                            "title": occ.event.title,
                            "start": occ.occurrence_start.format("%Y-%m-%dT%H:%M").to_string(),
                            "end": occ.occurrence_end.format("%Y-%m-%dT%H:%M").to_string(),
                            "calendar": occ.event.calendar,
                            "location": occ.event.location,
                        })
                    })
                    .collect();
                Ok(ToolResponse::success(&serde_json::json!({
                    "week_start": dates[0].to_string(),
                    "week_end": dates[6].to_string(),
                    "count": events.len(),
                    "events": events,
                })))
            }
            Err(e) => Ok(ToolResponse::error(&format!("Query failed: {e}"))),
        }
    }

    #[tool(description = "Set a reminder for an event (minutes before the event).")]
    async fn set_reminder(
        &self,
        Parameters(req): Parameters<SetReminderRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.store.read(&req.id) {
            Ok(mut event) => {
                if !event.reminders.contains(&req.minutes) {
                    event.reminders.push(req.minutes);
                }
                match self.store.update(&event) {
                    Ok(()) => Ok(ToolResponse::success(&serde_json::json!({
                        "id": req.id,
                        "title": event.title,
                        "reminders": event.reminders,
                    }))),
                    Err(e) => Ok(ToolResponse::error(&format!("Update failed: {e}"))),
                }
            }
            Err(e) => Ok(ToolResponse::error(&format!("Event not found: {e}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// ServerHandler
// ---------------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for KoyomiMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation {
                name: "koyomi".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                description: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Koyomi calendar MCP server. Manage calendar events, query schedules, \
                 and set reminders."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the MCP server on stdio.
pub async fn run(config: KoyomiConfig) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::{transport::stdio, ServiceExt};

    let service = KoyomiMcpServer::new(config).map_err(|e| e.to_string())?;
    let server = service.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
