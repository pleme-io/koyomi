# Koyomiban (暦盤) — GPU Calendar

> **★★★ CSE / Knowable Construction.** This repo operates under **Constructive Substrate Engineering** — canonical specification at [`pleme-io/theory/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md`](https://github.com/pleme-io/theory/blob/main/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md). The Compounding Directive (operational rules: solve once, load-bearing fixes only, idiom-first, models stay current, direction beats velocity) is in the org-level pleme-io/CLAUDE.md ★★★ section. Read both before non-trivial changes.


Crate: `kodate` | Binary: `koyomi` | Config app name: `koyomi`

GPU-rendered calendar with CalDAV sync, natural language event creation, and
vim-modal navigation. Uses kiroku (SeaORM) for local event cache and sakuin
(tantivy) for event search.

**Note:** The crate name is `kodate` in Cargo.toml (the name `koyomiban` was intended
for crates.io). The binary is `koyomi`. The config app name for shikumi is `koyomi`.

## Build & Test

```bash
cargo build                                         # compile
cargo test --lib                                     # unit tests
cargo test                                           # all tests
cargo run                                            # launch GUI
cargo run -- today                                   # show today's events
cargo run -- week                                    # show this week's events
cargo run -- add "Meeting" --start 2026-03-07T10:00 --end 2026-03-07T11:00
cargo run -- daemon                                  # start background sync daemon
```

Nix build:
```bash
nix build                     # build via substrate rust-tool-release-flake
nix run                       # run
nix run .#regenerate           # regenerate Cargo.nix after Cargo.toml changes
```

## Competitive Position

| Competitor | Stack | Our advantage |
|-----------|-------|---------------|
| **Fantastical** | macOS native | Cross-platform, open-source, MCP-drivable, Rhai scriptable |
| **GNOME Calendar** | C/GTK | GPU-rendered, vim-modal, plugin system, MCP automation |
| **calcurse** | C, TUI | GPU rendering, richer views, MCP-drivable |
| **khal** | Python, TUI | Native Rust performance, GPU rendering, Rhai plugins |
| **remind** | C, scripting | Full GUI with GPU rendering, similar scripting power via Rhai |

Unique value: GPU-rendered calendar with MCP for AI-driven scheduling, vim navigation,
natural language event input, and Rhai automation for recurring workflows.

## Architecture

### Module Map

```
src/
  main.rs                      ← CLI entry point (clap: open, today, week, add, daemon)
  lib.rs                       ← Library root (re-exports config + platform)
  config.rs                    ← KoyomiConfig via shikumi

  platform/
    mod.rs                     ← Platform trait definitions (CalendarBackend)
    macos/
      mod.rs                   ← macOS calendar backend

  calendar/                    ← (planned) Calendar data model
    mod.rs                     ← Calendar, Event, Recurrence, Alarm, Attendee
    event.rs                   ← Event struct (RFC 5545 compatible)
    recurrence.rs              ← RRULE parsing and expansion (RFC 5545 recurrence)
    alarm.rs                   ← Alarm/reminder definitions
    ical.rs                    ← iCalendar serialization/deserialization

  caldav/                      ← (planned) CalDAV sync engine
    mod.rs                     ← CalDavClient: discovery, sync, conflict resolution
    discovery.rs               ← .well-known/caldav, principal, calendar-home-set
    sync.rs                    ← REPORT, sync-token, ctag-based sync
    conflict.rs                ← Conflict resolution (server wins, client wins, merge)

  cache/                       ← (planned) Local event cache
    mod.rs                     ← EventCache: kiroku (SeaORM) backed SQLite
    schema.rs                  ← Database schema (events, calendars, sync_state)
    queries.rs                 ← Common queries (by date range, by calendar, search)

  views/                       ← (planned) Calendar views
    mod.rs                     ← ViewManager: month, week, day, agenda
    month.rs                   ← Month grid (7 columns, 5-6 rows)
    week.rs                    ← Week view (7 day columns, hourly rows)
    day.rs                     ← Day view (single column, hourly rows)
    agenda.rs                  ← Agenda view (chronological event list)

  input/                       ← (planned) Natural language event creation
    mod.rs                     ← NaturalLanguageParser
    parser.rs                  ← Parse "lunch with Bob tomorrow at noon" -> Event
    time.rs                    ← Relative time parsing ("next Tuesday", "in 2 hours")

  reminders/                   ← (planned) Notification scheduling
    mod.rs                     ← ReminderScheduler: tsuuchi integration
    scheduler.rs               ← Timer-based reminder dispatch

  render/                      ← (planned) GPU rendering
    mod.rs                     ← KoyomiRenderer: madori RenderCallback
    month_grid.rs              ← Month view GPU rendering (cells, events, today highlight)
    week_grid.rs               ← Week view rendering (time slots, event blocks)
    day_column.rs              ← Day view rendering (hour markers, event blocks)
    event_block.rs             ← Event block rendering (color, title, time)

  mcp/                         ← (planned) MCP server via kaname
    mod.rs                     ← KoyomiMcp server struct
    tools.rs                   ← Tool implementations

  scripting/                   ← (planned) Rhai scripting via soushi
    mod.rs                     ← Engine setup, koyomi.* API registration

module/
  default.nix                  ← HM module (blackmatter.components.koyomi)
```

### Data Flow

```
CalDAV Servers (Google, iCloud, Fastmail, self-hosted)
         │
         ▼
   CalDavClient (REPORT sync, conflict resolution)
         │
         ▼
   EventCache (SQLite via kiroku/SeaORM)
         │
         ├──▸ ViewManager ──▸ Month/Week/Day/Agenda views
         │                          │
         │                          ▼
         │                    GPU Render (garasu/madori/egaku)
         │                          │
         │                    Input Events (awase hotkeys)
         │
         ├──▸ ReminderScheduler ──▸ tsuuchi notifications
         │
         └──▸ Search Index (sakuin/tantivy) ──▸ event search
```

### Calendar Data Model (RFC 5545)

Core entities, compatible with iCalendar (RFC 5545):

- **Calendar** — `{id, name, color, source_url, enabled, sync_token}`
- **Event** — `{uid, calendar_id, summary, description, location, dtstart, dtend, rrule, alarms, attendees, status}`
- **Recurrence** — RRULE expansion (`FREQ=WEEKLY;BYDAY=MO,WE,FR;UNTIL=20261231`)
- **Alarm** — `{trigger: -PT15M, action: DISPLAY|AUDIO, description}`
- **Attendee** — `{email, name, role, status: ACCEPTED|DECLINED|TENTATIVE}`

### CalDAV Sync Strategy

1. **Discovery** — `.well-known/caldav` -> `current-user-principal` -> `calendar-home-set` -> calendar list
2. **Initial sync** — `REPORT calendar-multiget` to fetch all events
3. **Incremental sync** — `sync-token` based REPORT to fetch only changes since last sync
4. **Conflict resolution** — configurable: server-wins (default), client-wins, manual merge
5. **Offline mode** — all operations write to local cache first, sync when connectivity returns

### Current Implementation Status

**Done:**
- `config.rs` — shikumi integration with appearance/calendars/notifications/sync/daemon sections
- `platform/mod.rs` — CalendarBackend trait definition
- `platform/macos/mod.rs` — macOS calendar backend (basic structure)
- `main.rs` — CLI with today/week/add/daemon subcommands
- `lib.rs` — Library root
- `module/default.nix` — HM module (see flake.nix)
- `flake.nix` — substrate rust-tool-release-flake + HM module

**Not started:**
- GUI rendering via madori/garasu/egaku
- Calendar data model (RFC 5545 event/recurrence)
- CalDAV sync engine
- Local event cache (kiroku/SeaORM SQLite)
- Calendar views (month, week, day, agenda)
- Natural language event parsing
- Reminder scheduling via tsuuchi
- Event search via sakuin
- MCP server via kaname
- Rhai scripting via soushi
- Hotkey system via awase

## Configuration

Uses **shikumi** for config discovery and hot-reload:
- Config file: `~/.config/koyomi/koyomi.yaml`
- Env override: `$KOYOMI_CONFIG`
- Env prefix: `KOYOMI_` (e.g., `KOYOMI_APPEARANCE__WEEK_START=monday`)
- Hot-reload on file change (nix-darwin symlink aware)

### Config Schema

```yaml
appearance:
  width: 1000
  height: 700
  font_size: 14.0
  opacity: 0.95
  week_start: "monday"              # monday | sunday
  time_format: "24h"                # 24h | 12h
  default_view: "month"             # month | week | day | agenda

calendars:
  - name: "Personal"
    url: "https://caldav.example.com/dav/calendars/user/personal/"
    color: "#88c0d0"
    enabled: true
    username: "user"                 # CalDAV auth
    # password via KOYOMI_CALENDARS_0_PASSWORD env var or sops secret
  - name: "Work"
    url: "https://caldav.work.com/dav/"
    color: "#bf616a"
    enabled: true

notifications:
  enabled: true
  default_reminder_mins: 15
  sound: true

sync:
  interval_secs: 300                 # sync every 5 minutes
  offline_mode: false                # if true, never sync (local only)
  conflict_resolution: "server"      # server | client | manual

daemon:
  enable: false
  listen_addr: "127.0.0.1:9200"
  database_url: "sqlite://~/.local/share/koyomi/events.db"
```

## Shared Library Integration

| Library | Usage |
|---------|-------|
| **shikumi** | Config discovery + hot-reload (`KoyomiConfig`) |
| **sakuin** | Event search index (tantivy wrapper for full-text event search) |
| **kiroku** | Local event cache (SeaORM wrapper for SQLite persistence) |
| **garasu** | GPU rendering for calendar views |
| **madori** | App framework (event loop, render loop, timed refresh) |
| **egaku** | Widgets (grid for month, split pane, text input, modal for event editor) |
| **irodzuki** | Theme: base16 to GPU uniforms (calendar colors, today highlight) |
| **todoku** | HTTP client for CalDAV sync (replaces raw reqwest) |
| **tsunagu** | Daemon mode for background sync |
| **kaname** | MCP server framework |
| **soushi** | Rhai scripting engine |
| **awase** | Hotkey system for vim-modal navigation |
| **tsuuchi** | Notifications (event reminders, sync errors) |
| **hasami** | Clipboard (copy event details) |

## MCP Server (kaname)

Standard tools: `status`, `config_get`, `config_set`, `version`

App-specific tools:
- `list_events(date?, range_days?)` — list events for a date range
- `create_event(title, start, end, calendar?, location?, description?)` — create event
- `update_event(id, title?, start?, end?, location?)` — update event
- `delete_event(id)` — delete event
- `get_day(date)` — all events for a specific day
- `get_week(date?)` — all events for the week containing date
- `search_events(query)` — full-text search across all events
- `list_calendars()` — list configured calendars with sync status
- `sync()` — trigger CalDAV sync now
- `next_event()` — next upcoming event
- `free_busy(start, end)` — free/busy slots in a time range

## Rhai Scripting (soushi)

Scripts from `~/.config/koyomi/scripts/*.rhai`

```rhai
// Available API:
koyomi.today()                      // -> [{title, start, end, calendar}]
koyomi.events("2026-03-15")         // -> events for specific date
koyomi.events_range("2026-03-01", "2026-03-31")  // -> events in range
koyomi.create("Team standup", "2026-03-10T09:00", "2026-03-10T09:30")
koyomi.create_recurring("Standup", "09:00", "09:30", #{
    freq: "weekly",
    days: ["MO", "TU", "WE", "TH", "FR"],
})
koyomi.delete("event-uid-123")
koyomi.search("standup")            // -> matching events
koyomi.next()                       // -> next upcoming event
koyomi.sync()                       // trigger CalDAV sync
koyomi.view("week")                 // switch to week view
koyomi.remind("event-uid", 30)      // set 30-min reminder
koyomi.free_busy("09:00", "17:00")  // -> free slots today
```

Event hooks: `on_startup`, `on_shutdown`, `on_event_created(event)`,
`on_event_updated(event)`, `on_reminder(event)`, `on_sync_complete(changes)`

Example: auto-create lunch block on workdays:
```rhai
fn on_startup() {
    let today = koyomi.today();
    let has_lunch = today.iter().any(|e| e.title.contains("Lunch"));
    if !has_lunch && is_weekday() {
        koyomi.create("Lunch", "12:00", "13:00");
    }
}
```

## Hotkey System (awase)

### Modes

**Normal** (default — calendar grid navigation):
| Key | Action |
|-----|--------|
| `j/k` | Navigate days forward/backward |
| `h/l` | Previous/next week (in month view) |
| `H/L` | Previous/next month |
| `t` | Jump to today |
| `Enter` | Open day detail view |
| `a` | Add new event (opens event editor) |
| `v` | Cycle views (month -> week -> day -> agenda) |
| `1` | Month view |
| `2` | Week view |
| `3` | Day view |
| `4` | Agenda view |
| `s` | Sync now |
| `/` | Search events |
| `q` | Quit |
| `:` | Command mode |

**Day** (day detail view — viewing events in a day):
| Key | Action |
|-----|--------|
| `j/k` | Scroll time / navigate events |
| `a` | Add event at cursor time |
| `e` | Edit event under cursor |
| `d` | Delete event under cursor (confirm) |
| `Enter` | Open event detail |
| `Esc` | Back to calendar view |

**Event Editor** (creating/editing an event):
| Key | Action |
|-----|--------|
| `Tab` | Cycle fields (title, start, end, location, calendar, reminder) |
| `Enter` | Save event |
| `Esc` | Cancel |
| `i` | Enter insert mode for current field |

**Command** (`:` prefix):
- `:add <natural language>` — create event from natural language ("lunch tomorrow at noon")
- `:view month|week|day|agenda` — switch view
- `:sync` — trigger CalDAV sync
- `:search <query>` — search events
- `:goto <date>` — jump to date (YYYY-MM-DD)
- `:calendar <name> on|off` — toggle calendar visibility

## Nix Integration

### Flake Exports
- Multi-platform packages via substrate `rust-tool-release-flake.nix`
- `overlays.default` — `pkgs.koyomi`
- `homeManagerModules.default` — `blackmatter.components.koyomi`
- `devShells` — dev environment

### HM Module

Namespace: `blackmatter.components.koyomi`

Typed options:
- `enable` — install package + generate config
- `package` — override package
- `appearance.{width, height, font_size, opacity, week_start, time_format}`
- `calendars` — typed submodule list (name, url, color, enabled, username)
- `notifications.{enabled, default_reminder_mins, sound}`
- `sync.{interval_secs, offline_mode, conflict_resolution}`
- `daemon.{enable, listen_addr, database_url}` — launchd/systemd service
- `extraSettings` — raw attrset escape hatch

YAML generated via `lib.generators.toYAML` -> `xdg.configFile."koyomi/koyomi.yaml"`.
Uses substrate's `hm-service-helpers.nix` for `mkLaunchdService`/`mkSystemdService`.

**CalDAV credentials:** Never put passwords in the YAML config file. Use environment
variables (`KOYOMI_CALENDARS_0_PASSWORD`) or sops-encrypted secrets in the nix repo.

## Calendar View Design

### Month View

```
┌─────────────────── March 2026 ───────────────────┐
│  Mon    Tue    Wed    Thu    Fri    Sat    Sun    │
├──────┬──────┬──────┬──────┬──────┬──────┬────────┤
│      │      │      │      │      │      │   1    │
│      │      │      │      │      │      │        │
├──────┼──────┼──────┼──────┼──────┼──────┼────────┤
│   2  │   3  │   4  │   5  │   6  │   7  │   8    │
│      │ Meet │      │      │ Stan │      │        │
├──────┼──────┼──────┼──────┼──────┼──────┼────────┤
│  [9] │  10  │  11  │  12  │  13  │  14  │  15    │
│      │ Team │ Lunc │      │ Stan │      │        │
├──────┼──────┼──────┼──────┼──────┼──────┼────────┤
│  ... │      │      │      │      │      │        │
└──────┴──────┴──────┴──────┴──────┴──────┴────────┘
  [today]  event indicator: colored dot per calendar
```

- Each cell shows day number + truncated event titles (max 2-3, "+N more")
- Today highlighted with accent color from irodzuki theme
- Event color from calendar color in config
- Cursor navigation with `j/k/h/l`

### Week View

7 day columns, Y axis = hours (configurable work hours range).
Event blocks as colored rectangles spanning their duration.
Current time shown as a horizontal marker line.

### Day View

Single column, Y axis = hours. Full event detail visible (title, time, location).
Gaps between events clearly visible for scheduling.

## Design Constraints

- **CalDAV is the protocol** — no proprietary calendar APIs (Google Calendar API, etc.); CalDAV works with Google, iCloud, Fastmail, Radicale, all standards-compliant servers
- **Offline-first** — all operations write to local SQLite cache, sync is async and non-blocking
- **RFC 5545 compliance** — event model follows iCalendar standard for interop
- **No credential storage in config** — CalDAV passwords via env vars or sops secrets
- **Recurrence expansion** — RRULE expanded at query time, not stored as individual events
- **Natural language is advisory** — NLP event creation shows confirmation before saving
- **Reminder scheduling** — uses tsuuchi for desktop notifications, daemon must be running for reliable reminders
- **Time zones** — all internal storage in UTC, display in local timezone; chrono for all time handling
- **Sync conflicts** — default to server-wins; never silently discard user changes without logging
