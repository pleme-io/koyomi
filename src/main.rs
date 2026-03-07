mod config;
mod platform;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::config::KoyomiConfig;

#[derive(Parser)]
#[command(name = "koyomi", about = "Koyomi (暦) — calendar app")]
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
        /// Start time (ISO 8601).
        #[arg(long)]
        start: String,
        /// End time (ISO 8601).
        #[arg(long)]
        end: String,
    },
    /// Run the background daemon.
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
    let config = match shikumi::ConfigDiscovery::new("koyomi")
        .env_override("KOYOMI_CONFIG")
        .discover()
    {
        Ok(path) => {
            tracing::info!("loading config from {}", path.display());
            let store = shikumi::ConfigStore::<KoyomiConfig>::load(&path, "KOYOMI_")
                .unwrap_or_else(|e| {
                    tracing::warn!("failed to load config: {e}, using defaults");
                    let tmp = std::env::temp_dir().join("koyomi-default.yaml");
                    std::fs::write(&tmp, "{}").ok();
                    shikumi::ConfigStore::load(&tmp, "KOYOMI_").unwrap()
                });
            KoyomiConfig::clone(&store.get())
        }
        Err(_) => {
            tracing::info!("no config file found, using defaults");
            KoyomiConfig::default()
        }
    };

    let backend = platform::create_backend();

    match cli.command {
        Some(Command::Today) => {
            tracing::info!("showing today's events");
            let now = chrono::Local::now().naive_local();
            let start = now.date().and_hms_opt(0, 0, 0).unwrap();
            let end = now.date().and_hms_opt(23, 59, 59).unwrap();
            match backend.list_events(start, end) {
                Ok(events) => {
                    if events.is_empty() {
                        println!("No events today.");
                    } else {
                        for event in &events {
                            println!(
                                "{} - {} | {}",
                                event.start.format("%H:%M"),
                                event.end.format("%H:%M"),
                                event.title,
                            );
                        }
                    }
                }
                Err(e) => tracing::error!("failed to list events: {e}"),
            }
        }
        Some(Command::Week) => {
            tracing::info!("showing this week's events");
            let now = chrono::Local::now().naive_local();
            let start = now.date().and_hms_opt(0, 0, 0).unwrap();
            let end = start + chrono::Duration::days(7);
            match backend.list_events(start, end) {
                Ok(events) => {
                    if events.is_empty() {
                        println!("No events this week.");
                    } else {
                        for event in &events {
                            println!(
                                "{} {} - {} | {}",
                                event.start.format("%a"),
                                event.start.format("%H:%M"),
                                event.end.format("%H:%M"),
                                event.title,
                            );
                        }
                    }
                }
                Err(e) => tracing::error!("failed to list events: {e}"),
            }
        }
        Some(Command::Add { title, start, end }) => {
            tracing::info!("adding event: {title}");
            let start_dt = chrono::NaiveDateTime::parse_from_str(&start, "%Y-%m-%dT%H:%M")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(&start, "%Y-%m-%dT%H:%M:%S"))
                .expect("invalid start time format (expected ISO 8601)");
            let end_dt = chrono::NaiveDateTime::parse_from_str(&end, "%Y-%m-%dT%H:%M")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(&end, "%Y-%m-%dT%H:%M:%S"))
                .expect("invalid end time format (expected ISO 8601)");

            let event = platform::CalendarEvent {
                id: String::new(),
                title,
                description: None,
                start: start_dt,
                end: end_dt,
                location: None,
                calendar: String::from("default"),
                reminders: vec![config.notifications.default_reminder_mins],
            };

            match backend.create_event(&event) {
                Ok(id) => println!("Event created: {id}"),
                Err(e) => tracing::error!("failed to create event: {e}"),
            }
        }
        Some(Command::Daemon) => {
            tracing::info!("starting daemon on {}", config.daemon.listen_addr);
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(async {
                // TODO: implement daemon server
                tracing::info!("daemon running (not yet implemented)");
                tokio::signal::ctrl_c().await.ok();
            });
        }
        None => {
            // Default: launch GUI
            tracing::info!("launching GUI (not yet implemented)");
        }
    }
}
