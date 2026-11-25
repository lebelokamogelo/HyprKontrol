# Writing a theme

A theme is a TOML file. Everything in it is optional except `name`, so the
smallest useful theme changes a single colour.

```toml
name = "Ember"
base = "cachyos-dark"

[colors]
accent = "#ff8a3d"
on_accent = "#241000"
```

## Where themes live

```
~/.config/hyprkontrol/themes/
├── ember.toml
└── harbour/
    ├── theme.toml
    └── icons/
        └── wifi.svg
```

A file name, or a folder name when the theme brings its own icons, becomes the
theme's id. Ids of built-in themes are reserved.

Files are watched. Save an edit and the window follows within a second or two.

A file that cannot be read never stops the program. The Theme panel shows the
reason next to the theme's name, and the other themes keep working.

## Starting from the editor

Open **Theme** and choose **Copy and edit**. The editor writes the same kind of
file, holding only what you changed.

## Checking a theme

```console
$ hyprkontrol --check-theme ember.toml
Ember loads.
  Text on cards             13.6  ok
  Dim text on cards          8.0  ok
  Muted text on cards        5.9  ok
  Text on selected items    11.1  ok
  Text on the accent         7.8  ok
```

The numbers are WCAG contrast ratios. Text needs 4.5 or more. The exit code is 0
when everything passes, which makes the check usable in continuous integration.

## Top level

| Key | Meaning |
| --- | --- |
| `name` | Shown in the Theme panel. Required. |
| `author` | Shown below the name. |
| `base` | Id of the theme to inherit from. Defaults to `cachyos-dark`. |

A base can have a base of its own. Built-in ids are `cachyos-dark`,
`cachyos-light`, `glass`, `high-contrast` and `terminal`.

## `[colors]`

Colours are written as `#rgb`, `#rrggbb` or `#rrggbbaa`.

| Key | Used for |
| --- | --- |
| `background` | Behind every panel, and wells inside cards |
| `rail` | Icon strip on the left |
| `sidebar` | List of panels in a section |
| `card` | Groups of rows and tiles |
| `raised` | Selected and hovered items, secondary buttons |
| `track` | Empty part of sliders, switches and charts |
| `knob` | Switch handle when off |
| `line` | Dividers and borders, usually with a low alpha |
| `text` | Main text |
| `text_dim` | Values and descriptions |
| `text_mute` | Hints and captions |
| `accent` | Active controls |
| `on_accent` | Text and icons on top of the accent |
| `live` | Readings, charts and commands |
| `warning` | Things that need attention |
| `critical` | Errors and destructive actions |

### Translucent themes

Give the surfaces an alpha below `ff` and turn on blur in Hyprland. Start from
`glass`, which has values that stay readable over most wallpapers. Menus are
always opaque, since they float over other content.

Contrast is calculated as if surfaces were opaque. Check a translucent theme
against a light and a dark wallpaper.

## `[metrics]`

Lengths are in logical pixels. Values outside the range are clamped.

| Key | Range | Meaning |
| --- | --- | --- |
| `card_radius` | 0 to 32 | Corners of cards |
| `control_radius` | 0 to 24 | Corners of buttons, fields and menus. 0 also squares switches and discs |
| `card_border` | 0 to 3 | Border around cards, drawn in `line` |
| `density` | 0.75 to 1.4 | Multiplies row heights and padding |
| `gap` | 6 to 40 | Space between cards |
| `content_padding` | 12 to 64 | Space around a panel |
| `rail_width` | 60 to 120 | Width of the icon strip |
| `sidebar_width` | 200 to 380 | Width of the sidebar |
| `text_size` | 11 to 18 | Base size. Headings and captions follow it |
| `icon_size` | 14 to 28 | |
| `icon_stroke` | 1.0 to 2.5 | Line width of the built-in icons |

## `[fonts]`

| Key | Meaning |
| --- | --- |
| `ui` | Interface family. Unset follows the desktop font |
| `mono` | Family for commands and paths |

Use the family name that `fc-list : family` prints. A family without medium or
bold faces uses the closest one it has.

## `[layout]`

| Key | Meaning |
| --- | --- |
| `rail_labels` | Labels below the icons in the rail. `true` or `false` |

## `[icons]`

Replaces icons by name. Paths are relative to the theme file.

```toml
[icons]
wifi = "icons/wifi.svg"
chevron-right = "icons/next.svg"
```

Icons are recoloured by the program, so draw them in a single colour on a square
canvas. Names are the kebab-case form of the entries in
[`src/icon.rs`](../src/icon.rs): `Wifi` is `wifi` and `ChevronRight` is
`chevron-right`.

## Sharing

A theme without icons is a single file. Someone else installs it by pasting its
path into **Import** on the Theme panel, or by copying it into the themes folder.
