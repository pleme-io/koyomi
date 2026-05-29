{
  description = "Koyomi (暦) — calendar app for macOS and Linux";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.11";
    crate2nix.url = "github:nix-community/crate2nix";
    flake-utils.url = "github:numtide/flake-utils";
    substrate = {
      url = "github:pleme-io/substrate";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crate2nix,
    flake-utils,
    substrate,
  }:
    (import "${substrate}/lib/rust-tool-release-flake.nix" {
      inherit nixpkgs crate2nix flake-utils;
    }) {
      toolName = "kodate";
      src = self;
      repo = "pleme-io/koyomi";

      # Migration to substrate module-trio + shikumiTypedGroups.
      # See kekkai (template), hikki (enum + custom-gated daemon), and
      # shashin (Interactive processType). koyomi mirrors hikki's
      # "daemon-typed-group + custom gate" pattern, plus a calendars
      # list-of-submodules typed via raw lib.types in extraHmOptions.
      module = {
        # Override name so the option tree lives at
        # blackmatter.components.koyomi (not .kodate). The binary +
        # overlay attr remain `kodate` (toolName is the build artifact;
        # `module.name` is the option-tree leaf).
        name = "koyomi";
        description = "Koyomi (暦) — calendar app";
        hmNamespace = "blackmatter.components";
        binaryName = "kodate";
        packageAttr = "kodate";

        # Daemon wiring lives in extraHmConfigFn — the daemon-typed
        # group collides with the trio's withUserDaemon (both at
        # blackmatter.components.koyomi.daemon.*). Custom wiring lets
        # us keep the YAML key `daemon: { listen_addr, database_url }`
        # in the legacy format and gate via daemonEnable in
        # extraHmOptions.

        # Shikumi YAML config at ~/.config/kodate/kodate.yaml.
        withShikumiConfig = true;
        shikumiConfigPath = ".config/kodate/kodate.yaml";

        shikumiTypedGroups = {
          appearance = {
            width = {
              type = "int";
              default = 900;
              description = "Window width in pixels.";
            };
            height = {
              type = "int";
              default = 700;
              description = "Window height in pixels.";
            };
            font_size = {
              type = "float";
              default = 14.0;
              description = "Font size in points.";
            };
            opacity = {
              type = "float";
              default = 1.0;
              description = "Window opacity (0.0-1.0).";
            };
            week_start = {
              type = nixpkgs.lib.types.enum [ "monday" "sunday" ];
              default = "monday";
              description = "First day of the week.";
            };
            time_format = {
              type = nixpkgs.lib.types.enum [ "12h" "24h" ];
              default = "24h";
              description = "Time display format.";
            };
          };

          notifications = {
            enabled = {
              type = "bool";
              default = true;
              description = "Enable event notifications.";
            };
            default_reminder_mins = {
              type = "int";
              default = 15;
              description = "Default reminder in minutes before event.";
            };
            sound = {
              type = "bool";
              default = true;
              description = "Enable notification sound.";
            };
          };

          sync = {
            interval_secs = {
              type = "int";
              default = 300;
              description = "Sync interval in seconds.";
            };
            offline_mode = {
              type = "bool";
              default = false;
              description = "Enable offline mode (cache events locally).";
            };
          };

          daemon = {
            listen_addr = {
              type = "str";
              default = "0.0.0.0:50052";
              description = "Listen address for the daemon.";
            };
            database_url = {
              type = "str";
              default = "sqlite:///tmp/kodate/state.db";
              description = "Database URL for event storage (sqlite:// or postgres://).";
            };
          };
        };

        # Calendars list (CalDAV sources) + daemon-enable + escape hatch.
        # Calendars require a list-of-submodules type that doesn't fit
        # the typed-group alias dictionary; pass raw types here.
        extraHmOptions = {
          calendars = nixpkgs.lib.mkOption {
            type = nixpkgs.lib.types.listOf (nixpkgs.lib.types.submodule {
              options = {
                name = nixpkgs.lib.mkOption {
                  type = nixpkgs.lib.types.str;
                  description = "Human-readable calendar name.";
                };
                url = nixpkgs.lib.mkOption {
                  type = nixpkgs.lib.types.str;
                  description = "CalDAV URL for the calendar.";
                };
                color = nixpkgs.lib.mkOption {
                  type = nixpkgs.lib.types.nullOr nixpkgs.lib.types.str;
                  default = null;
                  description = "Display color (hex string).";
                };
                enabled = nixpkgs.lib.mkOption {
                  type = nixpkgs.lib.types.bool;
                  default = true;
                  description = "Whether this calendar is enabled.";
                };
              };
            });
            default = [ ];
            description = "CalDAV calendar sources.";
          };

          daemonEnable = nixpkgs.lib.mkOption {
            type = nixpkgs.lib.types.bool;
            default = false;
            description = ''
              Run kodate as a persistent daemon (launchd on macOS,
              systemd on Linux). The daemon syncs calendars in the
              background and serves events locally.
            '';
          };

          extraSettings = nixpkgs.lib.mkOption {
            type = nixpkgs.lib.types.attrs;
            default = { };
            description = "Additional raw settings merged on top of the typed YAML.";
          };
        };

        # Merge calendars + extraSettings into the YAML payload, and
        # wire the daemon (Interactive launchd / systemd-user).
        extraHmConfigFn = { cfg, pkgs, lib, config, ... }:
          let
            hmHelpers = import "${substrate}/lib/hm/service-helpers.nix" {
              inherit lib;
            };
            isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
            logDir =
              if isDarwin then "${config.home.homeDirectory}/Library/Logs"
              else "${config.home.homeDirectory}/.local/share/kodate/logs";
            calendarsList = map (cal:
              lib.filterAttrs (_: v: v != null) {
                inherit (cal) name url enabled color;
              }
            ) cfg.calendars;
            extras =
              (lib.optionalAttrs (calendarsList != [ ]) { calendars = calendarsList; })
              // cfg.extraSettings;
          in lib.mkMerge [
            (lib.mkIf (extras != { }) {
              services.koyomi.settings = extras;
            })

            {
              home.activation.kodate-log-dir =
                lib.hm.dag.entryAfter [ "writeBoundary" ] ''
                  run mkdir -p "${logDir}"
                '';
            }

            (lib.mkIf (cfg.daemonEnable && isDarwin)
              (hmHelpers.mkLaunchdService {
                name = "kodate";
                label = "io.pleme.kodate";
                command = "${cfg.package}/bin/kodate";
                args = [ "daemon" ];
                logDir = logDir;
                processType = "Interactive";
                keepAlive = true;
              }))

            (lib.mkIf (cfg.daemonEnable && !isDarwin)
              (hmHelpers.mkSystemdService {
                name = "kodate";
                description = "Kodate — calendar daemon";
                command = "${cfg.package}/bin/kodate";
                args = [ "daemon" ];
              }))
          ];
      };
    };
}
