# Configuration

Everything here can be changed from the **Theme** panel. The file is for people
who keep their configuration under version control.

## Preferences

`~/.config/hyprkontrol/config.toml`

```toml
theme = "cachyos-dark"

# Switch between two themes along with the desktop.
follow_desktop = false
light_theme = "cachyos-light"
dark_theme = "cachyos-dark"

# Panel shown at launch.
start = "home"

# Order of the sections. Any that are left out come last.
sections = ["home", "connect", "devices", "desktop", "cachyos", "system"]

# Panels that are not listed. Search still finds them.
hidden = ["printers"]

# Quick controls on Home, in order.
tiles = ["wifi", "bluetooth", "power", "firewall", "dnd", "night-light", "keep-awake", "scheduler"]

# The bar with the last command and Undo. Errors are always shown.
receipt = true
```

Panel names are printed by `hyprkontrol --list`. The available tiles are
`wifi`, `bluetooth`, `power`, `firewall`, `dnd`, `night-light`, `keep-awake`,
`scheduler`, `dark` and `mute`.

## Keyboard

| Keys | Action |
| --- | --- |
| <kbd>Ctrl</kbd> + <kbd>K</kbd> | Search |
| <kbd>Alt</kbd> + <kbd>1</kbd> to <kbd>6</kbd> | Open a section |
| <kbd>Tab</kbd>, <kbd>Shift</kbd> + <kbd>Tab</kbd> | Move between fields |
| <kbd>Enter</kbd> | Open the first search result |

A keybinding for the window itself, in Hyprland's Lua configuration:

```lua
hl.bind("SUPER + I", hl.dsp.exec_cmd("hyprkontrol"))
```

## Files it writes

No configuration is written until you change a setting. A file that already
exists is copied to `<name>.before-hyprkontrol` before its first edit.

| File | When |
| --- | --- |
| `~/.config/hyprkontrol/config.toml` | Preferences |
| `~/.config/hyprkontrol/themes/*.toml` | Saving or importing a theme |
| `~/.config/hyprkontrol/hyprland.json` | Any Hyprland setting |
| `~/.config/hypr/config/hyprkontrol.lua` | Generated from the file above |
| `~/.config/hypr/hyprland.lua` | One `require` line, once |
| `~/.config/hypr/hypridle.conf` | Idle timers |
| `~/.config/hypr/hyprpaper.conf` | Wallpaper |
| `~/.config/swaync/config.json` | Notifications |
| `~/.config/autostart/*.desktop` | Startup applications |
| `~/.cache/hyprkontrol/thumbnails/` | Wallpaper previews |
| `~/.cache/hyprkontrol/checked` | Checking for updates. Empty, only its date counts |

These need authorisation:

| File | When |
| --- | --- |
| `/etc/systemd/logind.conf.d/50-hyprkontrol.conf` | Lid and power button |
| `/etc/sysctl.d/99-hyprkontrol.conf` | Kernel tweaks |
| `/etc/locale.gen` | Adding a language |

Other settings are stored by the tool that owns them, such as NetworkManager,
GSettings or ufw.

## Turning the Hyprland overrides off

Remove this line from `~/.config/hypr/hyprland.lua` and reload:

```lua
require("config.hyprkontrol")
```
