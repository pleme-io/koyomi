# Koyomi (暦) — Calendar App

## Build & Test

```bash
cargo build                    # compile
cargo test --lib               # unit tests
cargo test                     # all tests
cargo run                      # launch GUI
cargo run -- today             # show today's events
cargo run -- week              # show this week's events
cargo run -- add "Meeting" --start 2026-03-07T10:00 --end 2026-03-07T11:00
cargo run -- daemon            # start background daemon
```

## Architecture

### Pipeline

```
CalDAV Server ──sync──▸ Local Cache (SQLite)
                              ↓
          CLI / GUI ──▸ CalendarBackend ──▸ Events
                              ↓
                     Notifications (reminders)
```

### Configuration

Uses **shikumi** for config discovery and hot-reload:
- Config file: `~/.config/koyomi/koyomi.yaml`
- Env override: `$KOYOMI_CONFIG`
- Env vars: `KOYOMI_` prefix (e.g. `KOYOMI_APPEARANCE__WIDTH=1200`)
- Hot-reload on file change (nix-darwin symlink aware)

### Platform Isolation (`src/platform/`)

| Trait | macOS Impl | Purpose |
|-------|------------|---------|
| `CalendarBackend` | `MacOSCalendarBackend` | CRUD operations on calendar events |

Linux implementations will be added under `src/platform/linux/`.

### Config Struct (`src/config.rs`)

| Section | Fields |
|---------|--------|
| `appearance` | `width`, `height`, `font_size`, `opacity`, `week_start`, `time_format` |
| `calendars` | list of `{ name, url, color, enabled }` |
| `notifications` | `enabled`, `default_reminder_mins`, `sound` |
| `sync` | `interval_secs`, `offline_mode` |
| `daemon` | `enable`, `listen_addr`, `database_url` |

## File Map

| Path | Purpose |
|------|---------|
| `src/config.rs` | Config struct (uses shikumi) |
| `src/platform/mod.rs` | Platform trait definitions + `CalendarBackend` |
| `src/platform/macos/mod.rs` | macOS calendar backend |
| `src/main.rs` | CLI entry point (clap subcommands) |
| `src/lib.rs` | Library root (re-exports config + platform) |
| `module/default.nix` | HM module with typed options + YAML generation |
| `flake.nix` | Nix flake (packages, overlay, HM module, devShell) |

## Design Decisions

### Configuration Language: YAML
- YAML is the primary and only configuration format
- Config file: `~/.config/koyomi/koyomi.yaml`
- Nix HM module generates YAML via `lib.generators.toYAML` from typed options
- `extraSettings` escape hatch for raw attrset merge

### CalDAV Support
- CalDAV is the primary calendar protocol (RFC 4791)
- Multiple calendar sources supported via `calendars` config list
- Offline mode caches events in local SQLite database
- Background sync via daemon mode

### Nix Integration
- Flake exports: `packages`, `overlays.default`, `homeManagerModules.default`, `devShells`
- HM module at `blackmatter.components.koyomi` with fully typed options
- Calendar sources as typed submodule list (not untyped attrs)
- YAML generated via `lib.generators.toYAML`
- Cross-platform: `mkLaunchdService` (macOS) + `mkSystemdService` (Linux)
- Uses substrate's `hm-service-helpers.nix` for service generation

### Cross-Platform Strategy
- Platform-specific calendar access: behind `CalendarBackend` trait
- macOS: EventKit / CalDAV
- Linux: (planned) GNOME Calendar / CalDAV
- Time handling: `chrono` crate (cross-platform)
- HTTP client: `reqwest` for CalDAV communication
