# Architecture

```
src/
├── main.rs      Command line and window setup
├── app.rs       Navigation, receipt bar, theme, subscriptions
├── config.rs    Preferences
├── icon.rs      Embedded icons
├── panels/      One module per panel
├── sys/         One module per subsystem, no UI
├── theme/       Theme format, library, widget styles
└── ui/          Building blocks shared by panels
```

Dependencies point one way: `app` → `panels` → `ui` and `sys`. `sys` knows
nothing about the interface and can be tested without a display.

## The life of a change

A panel never modifies the system. It describes what should happen and hands
that to the application.

1. A control sends a message to its panel.
2. The panel returns an `Effect` holding a `Change`: an operation and its inverse.
3. The application shows the operation in the receipt bar and runs it.
4. Whether it succeeded or not, the panel is loaded again.

The last step is why a switch that failed jumps back. Panels hold what the system
reports, so there is no second copy of the state to keep in step.

An operation (`sys::Op`) is one of five things: commands, Hyprland edits, a file
write, joining a network, or importing a VPN profile. Keeping it as data is what makes the receipt and
Undo possible.

## Reading

`Panel::load` returns a task that reads everything the panel shows. It runs when
the panel opens, after every change, and when the window regains focus.

`Panel::watch` says what keeps an open panel current: nothing, a timer, or a
command that prints a line when something happens. Prefer the command. Watching
stops when the window loses focus.

## Hyprland

`hyprctl eval` runs Lua in the compositor, so edits take effect at once. They are
then recorded in `hyprland.json`, from which `config/hyprkontrol.lua` is
generated.

The order is fixed: evaluate first, record second. A value Hyprland refuses must
never reach the generated file, where it would break the next login.

Undo distinguishes two cases. If the option was already overridden, the previous
override is restored. If it came from the user's own files, the override is
dropped, and their file is in charge again.

## Themes

`theme::spec` declares every colour and measurement once, in a macro that derives
the file format, the resolved theme and the editor. `theme::Theme` implements
iced's widget catalogs and is the only source of colour.

Views get a `Ui`, which carries the theme, the preferences and the available
width. Panels are composed from its methods and contain no styling of their own.

## Adding a panel

1. Write the backend in `src/sys/`, with tests for anything that parses text.
2. Create `src/panels/<name>.rs` with a `State` that implements `Panel`.
3. Add a line to the `panels!` list in `src/panels/mod.rs`:

   ```rust
   Fans, fans, "fans", "Fans", Devices, Gauge, "cooling speed curve temperature";
   ```

   These are the id, the module, the key for the command line, the title, the
   section, the icon, and words that search should find it by.

A minimal panel:

```rust
#[derive(Default)]
pub struct State {
    speed: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Option<u32>),
    Set(u32),
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(fans::speed(), Message::Loaded)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(speed) => {
                self.speed = speed;
                Effect::none()
            }
            Message::Set(speed) => Effect::change(
                Change::new(fans::set(speed)).undo(fans::set(self.speed.unwrap_or(0))),
            ),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        ui.panel("Fans", "", None, vec![ui.loading()])
    }
}
```

## Testing

```sh
make check
```

Panels are rendered without a window, at three widths, as part of the normal
tests. Four further tests need real hardware or a running desktop and are ignored by
default:

```sh
# Render panels with live data. Reads only.
SHOTS=target/shots cargo test screenshots -- --ignored

# Import a VPN profile and remove it. Use a throwaway file.
VPN_PROFILE=/path/to/test.ovpn cargo test imports_a_vendor_profile -- --ignored

# Turn Bluetooth on and put it back. Needs an adapter that is off.
cargo test powers_on_and_back -- --ignored

# Save a Hyprland option into a throwaway configuration.
XDG_CONFIG_HOME=$(mktemp -d) cargo test saves_to_a_sandbox -- --ignored
```

`ICED_TEST_BACKEND` selects `tiny-skia` or `wgpu`. Check both when drawing on a
canvas, since they differ in details such as clipping.
