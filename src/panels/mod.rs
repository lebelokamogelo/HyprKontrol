//! The settings panels.
//!
//! A panel owns the state of one screen and nothing else. It reads through
//! [`crate::sys`], and asks for changes by returning an [`Effect`]. The
//! application runs the change, shows it in the receipt bar and reloads the
//! panel afterwards, so panels hold what the system reports and not what they
//! hope it will become.
//!
//! To add a panel, create a module with a `State` that implements [`Panel`] and
//! list it in the `panels!` invocation below.

mod options;

use std::fmt::Debug;
use std::time::Duration;

use iced::Task;

use crate::config::Config;
use crate::icon::Icon;
use crate::sys::{self, Change, Cmd, Op};
use crate::theme::Spec;
use crate::ui::{Element, Ui};

/// What keeps a visible panel up to date. Hidden panels do no work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Watch {
    Never,
    Every(Duration),
    /// Reload whenever this command prints a line. Used for tools that can
    /// report events, which is both faster and cheaper than polling.
    Command(&'static [&'static str]),
}

/// Something only the application can do on a panel's behalf.
#[derive(Debug, Clone)]
pub enum Request {
    Open(Id),
    Configure(Box<Config>),
    /// Shows a theme without saving it.
    PreviewTheme(Box<Spec>),
    ReloadThemes,
    Report(sys::Error),
    Done(String),
}

/// The outcome of a panel update.
#[derive(Debug)]
pub struct Effect<M> {
    pub task: Task<M>,
    pub change: Option<Change>,
    pub request: Option<Request>,
}

impl<M> Effect<M> {
    pub fn none() -> Self {
        Self { task: Task::none(), change: None, request: None }
    }

    pub fn task(task: Task<M>) -> Self {
        Self { task, ..Self::none() }
    }

    pub fn change(change: Change) -> Self {
        Self { change: Some(change), ..Self::none() }
    }

    /// A change without an inverse.
    pub fn run(op: impl Into<Op>) -> Self {
        Self::change(Change::new(op))
    }

    pub fn request(request: Request) -> Self {
        Self { request: Some(request), ..Self::none() }
    }

    /// Runs a fallible change constructor, reporting its error.
    pub fn attempt(change: sys::Result<Change>) -> Self {
        match change {
            Ok(change) => Self::change(change),
            Err(error) => Self::request(Request::Report(error)),
        }
    }

    /// Runs a command whose arguments may have been rejected.
    pub fn checked(command: Option<Cmd>) -> Self {
        match command {
            Some(command) => Self::run(command),
            None => Self::request(Request::Report(sys::Error::new("That name is not valid"))),
        }
    }

    pub fn and(mut self, task: Task<M>) -> Self
    where
        M: Send + 'static,
    {
        self.task = Task::batch([self.task, task]);
        self
    }

    pub fn map<N: Send + 'static>(self, f: impl Fn(M) -> N + Clone + Send + 'static) -> Effect<N>
    where
        M: Send + 'static,
    {
        Effect { task: self.task.map(f), change: self.change, request: self.request }
    }
}

pub trait Panel: Default {
    type Message: Clone + Debug + Send + 'static;

    /// Reads everything the panel shows.
    fn load(&self) -> Task<Self::Message>;

    /// Reads what changes by itself. Called by [`Panel::watch`], and worth
    /// overriding when a full load is expensive.
    fn refresh(&self) -> Task<Self::Message> {
        self.load()
    }

    fn update(&mut self, message: Self::Message) -> Effect<Self::Message>;

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Self::Message>;

    /// Current state in a few words, shown in the sidebar.
    fn summary(&self) -> String {
        String::new()
    }

    fn watch(&self) -> Watch {
        Watch::Never
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Home,
    Connect,
    Devices,
    Desktop,
    Cachyos,
    System,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Home,
        Section::Connect,
        Section::Devices,
        Section::Desktop,
        Section::Cachyos,
        Section::System,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Section::Home => "home",
            Section::Connect => "connect",
            Section::Devices => "devices",
            Section::Desktop => "desktop",
            Section::Cachyos => "cachyos",
            Section::System => "system",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Home => "Home",
            Section::Connect => "Connect",
            Section::Devices => "Devices",
            Section::Desktop => "Desktop",
            Section::Cachyos => "CachyOS",
            Section::System => "System",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Section::Home => Icon::Home,
            Section::Connect => Icon::Wifi,
            Section::Devices => Icon::Monitor,
            Section::Desktop => Icon::Windows,
            Section::Cachyos => Icon::Cachy,
            Section::System => Icon::Sliders,
        }
    }

    /// Sections in the order the user prefers.
    pub fn ordered(config: &Config) -> Vec<Section> {
        let mut sections = Section::ALL.to_vec();
        sections.sort_by_key(|section| {
            let position = config.sections.iter().position(|key| key == section.key());
            position.unwrap_or(usize::MAX)
        });
        sections
    }

    /// The panels listed under this section.
    pub fn panels(self, config: &Config) -> impl Iterator<Item = Id> + '_ {
        Id::ALL
            .iter()
            .copied()
            .filter(move |id| id.section() == self)
            .filter(|id| !config.hidden.iter().any(|key| key == id.key()))
    }
}

macro_rules! panels {
    ($($id:ident, $module:ident, $key:literal, $title:literal, $section:ident, $icon:ident, $terms:literal;)*) => {
        $(pub mod $module;)*

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Id { $($id),* }

        impl Id {
            pub const ALL: &[Id] = &[$(Id::$id),*];

            /// Stable name used in preferences and on the command line.
            pub fn key(self) -> &'static str {
                match self { $(Id::$id => $key),* }
            }

            pub fn title(self) -> &'static str {
                match self { $(Id::$id => $title),* }
            }

            pub fn section(self) -> Section {
                match self { $(Id::$id => Section::$section),* }
            }

            pub fn icon(self) -> Icon {
                match self { $(Id::$id => Icon::$icon),* }
            }

            /// Extra words that search should find this panel by.
            fn terms(self) -> &'static str {
                match self { $(Id::$id => $terms),* }
            }
        }

        #[derive(Debug, Clone)]
        pub enum Message { $($id(<$module::State as Panel>::Message)),* }

        impl Message {
            pub fn id(&self) -> Id {
                match self { $(Message::$id(_) => Id::$id),* }
            }
        }

        #[derive(Default)]
        pub struct Panels { $(pub $module: $module::State),* }

        impl Panels {
            pub fn load(&self, id: Id) -> Task<Message> {
                match id { $(Id::$id => self.$module.load().map(Message::$id)),* }
            }

            pub fn refresh(&self, id: Id) -> Task<Message> {
                match id { $(Id::$id => self.$module.refresh().map(Message::$id)),* }
            }

            pub fn update(&mut self, message: Message) -> Effect<Message> {
                match message {
                    $(Message::$id(message) => {
                        self.$module.update(message).map(Message::$id)
                    }),*
                }
            }

            pub fn view<'a>(&'a self, id: Id, ui: Ui<'a>) -> Element<'a, Message> {
                match id { $(Id::$id => self.$module.view(ui).map(Message::$id)),* }
            }

            pub fn summary(&self, id: Id) -> String {
                match id { $(Id::$id => self.$module.summary()),* }
            }

            pub fn watch(&self, id: Id) -> Watch {
                match id { $(Id::$id => self.$module.watch()),* }
            }
        }
    };
}

panels! {
    Home, home, "home", "Overview", Home, Home, "home start quick toggles";
    Network, network, "network", "Network", Connect, Wifi, "wifi wireless ethernet wired internet ip dns airplane";
    Bluetooth, bluetooth, "bluetooth", "Bluetooth", Connect, Bluetooth, "pair headset headphones";
    Vpn, vpn, "vpn", "VPN", Connect, Vpn, "wireguard openvpn tunnel";
    Hotspot, hotspot, "hotspot", "Hotspot", Connect, Hotspot, "tether share internet access point";
    Power, power, "power", "Power and battery", Devices, Battery, "energy profile performance suspend lid charge health";
    Displays, displays, "displays", "Displays", Devices, Monitor, "screen resolution refresh rate scale rotate brightness monitor";
    Sound, sound, "sound", "Sound", Devices, Sound, "audio volume microphone speaker output input mute";
    Pointer, pointer, "pointer", "Mouse and touchpad", Devices, Mouse, "cursor speed scroll natural tap acceleration";
    Keyboard, keyboard, "keyboard", "Keyboard", Devices, Keyboard, "layout language repeat numlock caps variant";
    Storage, storage, "storage", "Storage", Devices, Disk, "disk space cache clean trim journal";
    Printers, printers, "printers", "Printers", Devices, Printer, "cups print scan";
    Theme, theme, "theme", "Theme", Desktop, Contrast, "colors colours accent customise customize appearance this app layout";
    Appearance, appearance, "appearance", "Appearance", Desktop, Sun, "dark light gtk icons cursor fonts accent text size";
    Wallpaper, wallpaper, "wallpaper", "Wallpaper", Desktop, Image, "background picture hyprpaper";
    Windows, windows, "windows", "Windows and gaps", Desktop, Windows, "hyprland border rounding blur opacity layout dwindle master shadow";
    Animations, animations, "animations", "Animations", Desktop, Wave, "hyprland motion speed bezier curve";
    Shortcuts, shortcuts, "shortcuts", "Shortcuts", Desktop, Keyboard, "keybinds bindings hotkeys keys";
    Notifications, notifications, "notifications", "Notifications", Desktop, Bell, "swaync do not disturb popup timeout";
    Idle, idle, "idle", "Lock and idle", Desktop, Lock, "hypridle hyprlock sleep dim screen off";
    Apps, apps, "apps", "Apps and startup", Desktop, Apps, "default browser autostart login open with";
    Kernels, kernels, "kernels", "Kernels", Cachyos, Cachy, "linux bore lts hardened install";
    Scheduler, scheduler, "scheduler", "Scheduler", Cachyos, Chip, "scx sched-ext bpfland lavd cpu gaming";
    Updates, updates, "updates", "Updates", Cachyos, Package, "upgrade pacman packages";
    Mirrors, mirrors, "mirrors", "Mirrors", Cachyos, Globe, "rank speed download server";
    Drivers, drivers, "drivers", "Drivers", Cachyos, Gpu, "chwd graphics nvidia intel amd hardware";
    Tweaks, tweaks, "tweaks", "Tweaks", Cachyos, Sliders, "ananicy zram swappiness sysctl performance";
    Accounts, accounts, "accounts", "Accounts", System, User, "users password administrator shell name";
    Time, time, "time", "Date and time", System, Clock, "timezone clock ntp";
    Language, language, "language", "Language", System, Language, "locale region formats";
    Firewall, firewall, "firewall", "Firewall", System, Shield, "ufw ports rules security";
    Services, services, "services", "Services", System, Services, "systemd units daemon start stop enable logs";
    About, about, "about", "About", System, Info, "hostname hardware version kernel system information";
}

impl Id {
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|id| id.key() == key)
    }

    /// Panels that match every word of `query`, best matches first.
    pub fn search(query: &str) -> Vec<Id> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut matches: Vec<(usize, Id)> = Id::ALL
            .iter()
            .filter_map(|id| {
                let title = id.title().to_lowercase();
                let text =
                    format!("{title} {} {}", id.section().title().to_lowercase(), id.terms());
                words.iter().all(|word| text.contains(word)).then(|| {
                    // A hit at the start of the title beats one inside it, and
                    // both beat a hit in the search terms.
                    let rank = match title.find(&words[0]) {
                        Some(0) => 0,
                        Some(_) => 1,
                        None => 2,
                    };
                    (rank, *id)
                })
            })
            .collect();
        matches.sort_by_key(|(rank, _)| *rank);
        matches.into_iter().map(|(_, id)| id).collect()
    }
}

/// Runs a command without a receipt, for continuous changes such as dragging a
/// volume slider. `done` is sent when it finishes.
pub(crate) fn quietly<M: Send + 'static>(command: Cmd, done: M) -> Task<M> {
    Task::future(async move {
        let _ = command.run().await;
        done
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_round_trip() {
        for id in Id::ALL {
            assert_eq!(Id::from_key(id.key()), Some(*id));
        }
    }

    #[test]
    fn search_prefers_titles() {
        assert_eq!(Id::search("sound").first(), Some(&Id::Sound));
        assert_eq!(Id::search("volume").first(), Some(&Id::Sound));
        assert!(Id::search("gaps hyprland").contains(&Id::Windows));
        assert!(Id::search("").is_empty());
        assert!(Id::search("zzz").is_empty());
    }

    #[test]
    fn preferences_hide_panels_and_reorder_sections() {
        let mut config = Config { hidden: vec!["printers".into()], ..Config::default() };
        assert!(!Section::Devices.panels(&config).any(|id| id == Id::Printers));
        config.sections = vec!["system".into()];
        let order = Section::ordered(&config);
        assert_eq!(order.len(), Section::ALL.len());
        assert_eq!(order[0], Section::System);
    }
}
