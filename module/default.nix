# Koyomi home-manager module — calendar app with typed config + daemon
#
# Namespace: blackmatter.components.koyomi.*
#
# Generates YAML config from typed Nix options, loaded by shikumi at runtime.
# Supports hot-reload via symlink-aware file watching.
#
# Module factory: receives { hmHelpers } from flake.nix, returns HM module.
{ hmHelpers }:
{
  lib,
  config,
  pkgs,
  ...
}:
with lib;
let
  inherit (hmHelpers) mkLaunchdService mkSystemdService;
  cfg = config.blackmatter.components.koyomi;
  isDarwin = pkgs.stdenv.isDarwin;

  logDir =
    if isDarwin then "${config.home.homeDirectory}/Library/Logs"
    else "${config.home.homeDirectory}/.local/share/kodate/logs";

  # -- YAML config generation --------------------------------------------------
  settingsAttr = let
    appearance = filterAttrs (_: v: v != null) {
      inherit (cfg.appearance) width height font_size opacity week_start time_format;
    };

    calendars = map (cal: filterAttrs (_: v: v != null) {
      inherit (cal) name url enabled;
      color = cal.color;
    }) cfg.calendars;

    notifications = filterAttrs (_: v: v != null) {
      inherit (cfg.notifications) enabled default_reminder_mins sound;
    };

    sync = filterAttrs (_: v: v != null) {
      inherit (cfg.sync) interval_secs offline_mode;
    };

    daemon = optionalAttrs cfg.daemon.enable (filterAttrs (_: v: v != null) {
      listen_addr = cfg.daemon.listen_addr;
      database_url = cfg.daemon.database_url;
    });
  in
    filterAttrs (_: v: v != {} && v != null && v != []) {
      inherit appearance notifications sync daemon;
    }
    // optionalAttrs (calendars != []) { inherit calendars; }
    // cfg.extraSettings;

  yamlConfig = pkgs.writeText "kodate.yaml"
    (lib.generators.toYAML { } settingsAttr);
in
{
  options.blackmatter.components.koyomi = {
    enable = mkEnableOption "Koyomi — calendar app";

    package = mkOption {
      type = types.package;
      default = pkgs.kodate;
      description = "The kodate package to use.";
    };

    # -- Appearance ------------------------------------------------------------
    appearance = {
      width = mkOption {
        type = types.int;
        default = 900;
        description = "Window width in pixels.";
      };

      height = mkOption {
        type = types.int;
        default = 700;
        description = "Window height in pixels.";
      };

      font_size = mkOption {
        type = types.float;
        default = 14.0;
        description = "Font size in points.";
      };

      opacity = mkOption {
        type = types.float;
        default = 1.0;
        description = "Window opacity (0.0-1.0).";
      };

      week_start = mkOption {
        type = types.enum [ "monday" "sunday" ];
        default = "monday";
        description = "First day of the week.";
      };

      time_format = mkOption {
        type = types.enum [ "12h" "24h" ];
        default = "24h";
        description = "Time display format.";
      };
    };

    # -- Calendars -------------------------------------------------------------
    calendars = mkOption {
      type = types.listOf (types.submodule {
        options = {
          name = mkOption {
            type = types.str;
            description = "Human-readable calendar name.";
          };

          url = mkOption {
            type = types.str;
            description = "CalDAV URL for the calendar.";
          };

          color = mkOption {
            type = types.nullOr types.str;
            default = null;
            description = "Display color (hex string).";
          };

          enabled = mkOption {
            type = types.bool;
            default = true;
            description = "Whether this calendar is enabled.";
          };
        };
      });
      default = [];
      description = "CalDAV calendar sources.";
    };

    # -- Notifications ---------------------------------------------------------
    notifications = {
      enabled = mkOption {
        type = types.bool;
        default = true;
        description = "Enable event notifications.";
      };

      default_reminder_mins = mkOption {
        type = types.int;
        default = 15;
        description = "Default reminder in minutes before event.";
      };

      sound = mkOption {
        type = types.bool;
        default = true;
        description = "Enable notification sound.";
      };
    };

    # -- Sync ------------------------------------------------------------------
    sync = {
      interval_secs = mkOption {
        type = types.int;
        default = 300;
        description = "Sync interval in seconds.";
      };

      offline_mode = mkOption {
        type = types.bool;
        default = false;
        description = "Enable offline mode (cache events locally).";
      };
    };

    # -- Daemon ----------------------------------------------------------------
    daemon = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = ''
          Run kodate as a persistent daemon (launchd on macOS, systemd on Linux).
          The daemon syncs calendars in the background and serves events locally.
        '';
      };

      listen_addr = mkOption {
        type = types.str;
        default = "0.0.0.0:50052";
        description = "Listen address for the daemon.";
      };

      database_url = mkOption {
        type = types.str;
        default = "sqlite:///tmp/kodate/state.db";
        description = "Database URL for event storage (sqlite:// or postgres://).";
      };
    };

    # -- Escape hatch ----------------------------------------------------------
    extraSettings = mkOption {
      type = types.attrs;
      default = {};
      description = ''
        Additional raw settings merged on top of typed options.
        Use this for experimental or newly-added config keys not yet
        covered by typed options. Values are serialized directly to YAML.
      '';
    };
  };

  config = mkIf cfg.enable (mkMerge [
    # Install the package
    {
      home.packages = [ cfg.package ];
    }

    # Create log directory
    {
      home.activation.kodate-log-dir = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
        run mkdir -p "${logDir}"
      '';
    }

    # YAML configuration -- always generated from typed options
    {
      xdg.configFile."kodate/kodate.yaml".source = yamlConfig;
    }

    # Darwin: launchd agent (daemon mode)
    (mkIf (cfg.daemon.enable && isDarwin)
      (mkLaunchdService {
        name = "kodate";
        label = "io.pleme.kodate";
        command = "${cfg.package}/bin/kodate";
        args = [ "daemon" ];
        logDir = logDir;
        processType = "Interactive";
        keepAlive = true;
      })
    )

    # Linux: systemd user service (daemon mode)
    (mkIf (cfg.daemon.enable && !isDarwin)
      (mkSystemdService {
        name = "kodate";
        description = "Kodate — calendar daemon";
        command = "${cfg.package}/bin/kodate";
        args = [ "daemon" ];
      })
    )
  ]);
}
