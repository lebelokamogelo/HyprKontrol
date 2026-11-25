use std::collections::VecDeque;
use std::time::Duration;

use iced::widget::{button, column, container, row, space};
use iced::{Center, Fill, Length, Task};

use super::{Effect, Id, Panel, Request, Section, Watch, quietly};
use crate::icon::Icon;
use crate::sys::{self, audio, desktop, network, packages, power, probe, services, storage};
use crate::theme::style::{self, Tone};
use crate::ui::{Element, Throttle, Ui, chart};

/// Number of samples on the clock chart, one per second.
const HISTORY: usize = 40;
/// Everything else on this panel changes slowly and is read less often.
const REFRESH: u32 = 10;

/// A quick control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Wifi,
    Bluetooth,
    Power,
    Firewall,
    Dnd,
    NightLight,
    KeepAwake,
    Scheduler,
    Dark,
    Mute,
}

impl Tile {
    pub const ALL: [Tile; 10] = [
        Tile::Wifi,
        Tile::Bluetooth,
        Tile::Power,
        Tile::Firewall,
        Tile::Dnd,
        Tile::NightLight,
        Tile::KeepAwake,
        Tile::Scheduler,
        Tile::Dark,
        Tile::Mute,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Tile::Wifi => "wifi",
            Tile::Bluetooth => "bluetooth",
            Tile::Power => "power",
            Tile::Firewall => "firewall",
            Tile::Dnd => "dnd",
            Tile::NightLight => "night-light",
            Tile::KeepAwake => "keep-awake",
            Tile::Scheduler => "scheduler",
            Tile::Dark => "dark",
            Tile::Mute => "mute",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tile::Wifi => "Wi-Fi",
            Tile::Bluetooth => "Bluetooth",
            Tile::Power => "Power mode",
            Tile::Firewall => "Firewall",
            Tile::Dnd => "Do not disturb",
            Tile::NightLight => "Night light",
            Tile::KeepAwake => "Keep awake",
            Tile::Scheduler => "Scheduler",
            Tile::Dark => "Dark mode",
            Tile::Mute => "Mute",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Tile::Wifi => Icon::Wifi,
            Tile::Bluetooth => Icon::Bluetooth,
            Tile::Power => Icon::Gauge,
            Tile::Firewall => Icon::Shield,
            Tile::Dnd => Icon::Dnd,
            Tile::NightLight => Icon::Moon,
            Tile::KeepAwake => Icon::Eye,
            Tile::Scheduler => Icon::Chip,
            Tile::Dark => Icon::Contrast,
            Tile::Mute => Icon::Muted,
        }
    }
}

/// What the tiles and levels show. Read every few seconds.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    network: Option<network::Network>,
    bluetooth: Option<bool>,
    profiles: power::Profiles,
    firewall: bool,
    dnd: Option<bool>,
    night_light: Option<bool>,
    idle_active: bool,
    scheduler: packages::Scheduler,
    look: desktop::Look,
    brightness: Option<f32>,
    audio: Option<audio::Audio>,
}

/// What does not change while one looks at it. Read when the panel opens.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    root: Option<storage::Volume>,
    updates: usize,
    last_sync: Option<String>,
    machine: probe::Machine,
    topology: probe::Topology,
}

async fn snapshot() -> Snapshot {
    let (
        (network, bluetooth, profiles, dnd),
        (scheduler, look, brightness, audio),
        (idle_active, night_light),
    ) = tokio::join!(
        async {
            tokio::join!(
                network::load(),
                sys::query("bluetoothctl", ["show"]),
                power::profiles(),
                desktop::do_not_disturb()
            )
        },
        async {
            tokio::join!(packages::scheduler(), desktop::look(), power::brightness(), audio::load())
        },
        async {
            tokio::join!(
                services::is_active(services::Scope::User, "hypridle.service"),
                night_light(),
            )
        },
    );
    Snapshot {
        network: network.ok(),
        bluetooth: bluetooth.map(|text| text.contains("Powered: yes")),
        profiles,
        firewall: sys::ufw::load().enabled,
        dnd,
        night_light,
        idle_active,
        scheduler,
        look,
        brightness,
        audio: audio.ok(),
    }
}

async fn facts() -> Facts {
    let (disks, updates, machine) =
        tokio::join!(storage::disks(), packages::cached_updates(), probe::machine());
    Facts {
        root: disks.ok().and_then(|disks| storage::root(&disks).cloned()),
        updates: updates.len(),
        last_sync: packages::last_sync().map(packages::ago),
        machine,
        topology: probe::topology(),
    }
}

/// `None` when hyprsunset is not installed.
async fn night_light() -> Option<bool> {
    if !sys::installed("hyprsunset") {
        return None;
    }
    Some(sys::query("pgrep", ["-x", "hyprsunset"]).await.is_some())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Brightness,
    Speaker,
    Microphone,
}

#[derive(Default)]
pub struct State {
    snapshot: Option<Snapshot>,
    facts: Option<Facts>,
    sample: probe::Sample,
    load: f32,
    clock: VecDeque<f32>,
    battery: Option<power::Battery>,
    ticks: u32,
    query: String,
    dragging: Option<(Level, f32)>,
    throttle: Throttle<(Level, f32)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Snapshot>),
    Facts(Box<Facts>),
    Sampled(Box<(probe::Sample, Option<power::Battery>)>),
    Search(String),
    Open(Id),
    OpenFirst,
    Press(Tile),
    Drag(Level, f32),
    Release(Level),
    Sent,
}

/// The search field, focused by the application on Ctrl+K.
pub const SEARCH: &str = "home-search";

impl State {
    fn command(&self, level: Level, percent: f32) -> Option<sys::Cmd> {
        let audio = self.snapshot.as_ref()?.audio.as_ref();
        match level {
            Level::Brightness => Some(power::set_brightness(percent)),
            Level::Speaker => audio?
                .output()
                .map(|d| audio::set_volume(audio::Direction::Output, &d.name, percent)),
            Level::Microphone => audio?
                .input()
                .map(|d| audio::set_volume(audio::Direction::Input, &d.name, percent)),
        }
    }

    fn level(&self, level: Level) -> Option<f32> {
        if let Some((dragged, percent)) = self.dragging
            && dragged == level
        {
            return Some(percent);
        }
        let snapshot = self.snapshot.as_ref()?;
        let audio = snapshot.audio.as_ref();
        match level {
            Level::Brightness => snapshot.brightness,
            Level::Speaker => audio?.output().map(|device| device.volume),
            Level::Microphone => audio?.input().map(|device| device.volume),
        }
    }

    fn press(&self, tile: Tile) -> Effect<Message> {
        let Some(s) = &self.snapshot else { return Effect::none() };
        match tile {
            Tile::Wifi => match &s.network {
                Some(network) => Effect::change(
                    sys::Change::new(network::set_wifi(!network.wifi_enabled))
                        .undo(network::set_wifi(network.wifi_enabled)),
                ),
                None => Effect::request(Request::Open(Id::Network)),
            },
            Tile::Bluetooth => match s.bluetooth {
                Some(on) => Effect::change(sys::bluetooth::set_power(
                    !on,
                    sys::bluetooth::Block::current(),
                )),
                None => Effect::request(Request::Open(Id::Bluetooth)),
            },
            Tile::Power => {
                let available = &s.profiles.available;
                let at = available.iter().position(|name| *name == s.profiles.active);
                match at {
                    Some(at) => {
                        let next = &available[(at + 1) % available.len()];
                        Effect::change(power::set_profile(&s.profiles.active, next))
                    }
                    None => Effect::request(Request::Open(Id::Power)),
                }
            }
            Tile::Firewall => Effect::change(sys::ufw::set_enabled(!s.firewall)),
            Tile::Dnd => match s.dnd {
                Some(on) => Effect::change(desktop::set_do_not_disturb(!on)),
                None => Effect::request(Request::Open(Id::Notifications)),
            },
            Tile::NightLight => match s.night_light {
                Some(true) => Effect::run(sys::Cmd::new("pkill").args(["-x", "hyprsunset"])),
                Some(false) => Effect::run(
                    sys::Cmd::new("hyprsunset").args(["--temperature", "4500"]).detached(),
                ),
                None => Effect::checked(packages::install(&["hyprsunset"])),
            },
            Tile::KeepAwake => {
                Effect::change(services::set_active(
                    services::Scope::User,
                    "hypridle.service",
                    !s.idle_active,
                ))
            }
            Tile::Scheduler => Effect::request(Request::Open(Id::Scheduler)),
            Tile::Dark => Effect::change(desktop::set_dark(&s.look, !s.look.dark)),
            Tile::Mute => match s.audio.as_ref().and_then(audio::Audio::output) {
                Some(device) => {
                    let set =
                        |on| audio::set_mute(audio::Direction::Output, &device.name, on);
                    Effect::change(sys::Change::new(set(!device.muted)).undo(set(device.muted)))
                }
                None => Effect::request(Request::Open(Id::Sound)),
            },
        }
    }

    /// Whether the tile is lit, and its state in words.
    fn tile_state(&self, tile: Tile) -> (bool, String) {
        let on_off = |on: bool| (on, if on { "On" } else { "Off" }.to_owned());
        let missing = |text: &str| (false, text.to_owned());
        let Some(s) = &self.snapshot else { return missing("") };
        match tile {
            Tile::Wifi => match &s.network {
                Some(n) if !n.wifi_present => missing("No adapter"),
                Some(n) if n.active_wifi().is_some() => (true, "Connected".to_owned()),
                Some(n) if n.wifi_enabled => (true, "Not connected".to_owned()),
                Some(_) => on_off(false),
                None => missing("Unavailable"),
            },
            Tile::Bluetooth => s.bluetooth.map_or(missing("No adapter"), on_off),
            Tile::Power => match s.profiles.active.as_str() {
                "" => missing("Unavailable"),
                "performance" => (true, "Performance".to_owned()),
                "power-saver" => (true, "Power saver".to_owned()),
                _ => (true, "Balanced".to_owned()),
            },
            Tile::Firewall => on_off(s.firewall),
            Tile::Dnd => s.dnd.map_or(missing("Unavailable"), on_off),
            Tile::NightLight => s.night_light.map_or(missing("Not installed"), on_off),
            Tile::KeepAwake => on_off(!s.idle_active),
            Tile::Scheduler => match &s.scheduler.running {
                Some((name, _)) => (true, name.clone()),
                None => missing("Kernel default"),
            },
            Tile::Dark => on_off(s.look.dark),
            Tile::Mute => on_off(
                s.audio.as_ref().and_then(audio::Audio::output).is_some_and(|d| d.muted),
            ),
        }
    }

    fn tile<'a>(&self, ui: Ui<'a>, tile: Tile) -> Element<'a, Message> {
        let (on, state) = self.tile_state(tile);
        let tone = if on { Tone::OnAccent } else { Tone::Dim };
        let disc = container(ui.icon(tile.icon(), tone))
            .center(42)
            .style(style::disc(on, 42.0));
        let labels = column![
            ui.bold(tile.label()).wrapping(iced::widget::text::Wrapping::None),
            ui.small(state).wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(1);
        button(row![disc, labels].spacing(12).align_y(Center))
            .padding([ui.d(14.0), 14.0])
            .width(Fill)
            .clip(true)
            .style(style::tile)
            .on_press(Message::Press(tile))
            .into()
    }

    fn levels<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        let rows = [
            (Level::Brightness, Icon::Sun),
            (Level::Speaker, Icon::Sound),
            (Level::Microphone, Icon::Mic),
        ]
        .into_iter()
        .filter_map(|(level, icon)| {
            let percent = self.level(level)?;
            let drag = move |percent| Message::Drag(level, percent);
            Some(ui.icon_slide(icon, percent.min(100.0), drag, Message::Release(level)))
        })
        .collect();
        ui.group("Levels", rows)
    }

    fn processor<'a>(&self, ui: Ui<'a>, facts: &Facts) -> Element<'a, Message> {
        let max = facts.topology.max_ghz;
        let header = row![
            ui.caption("Average clock").width(Fill),
            ui.small(format!("of {max:.1} GHz")),
        ];
        let spark = chart::Spark {
            values: self.clock.iter().copied().collect(),
            max,
            span: HISTORY,
        };
        let load = format!("{:.0}% busy", self.load * 100.0);
        let detail = match self.sample.celsius {
            Some(celsius) => format!("{celsius:.0} °C, {load}"),
            None => load,
        };
        let footer = row![
            ui.heading(format!("{:.2} GHz", self.sample.ghz)).width(Fill),
            ui.small(detail),
        ]
        .align_y(Center);
        ui.card(column![header, spark.view(ui.d(112.0)), footer].spacing(12))
    }

    fn notes<'a>(
        &self,
        ui: Ui<'a>,
        snapshot: &Snapshot,
        facts: &Facts,
    ) -> Element<'a, Message> {
        let mut rows = Vec::new();
        if let Some(battery) = &self.battery
            && battery.health() < 0.8
        {
            let cycles =
                battery.cycles.map_or(String::new(), |cycles| format!("{cycles} charge cycles"));
            rows.push(ui.alert(
                Icon::BatteryHealth,
                format!("Battery holds {:.0}% of its original charge", battery.health() * 100.0),
                cycles,
                Message::Open(Id::Power),
            ));
        }
        if let Some(root) = &facts.root
            && root.fraction() > 0.9
        {
            rows.push(ui.alert(
                Icon::DiskFull,
                "The system disk is almost full",
                format!("{} left", sys::bytes(root.free())),
                Message::Open(Id::Storage),
            ));
        }
        if !snapshot.firewall {
            rows.push(ui.alert(
                Icon::ShieldOff,
                "The firewall is off",
                "Incoming connections are not filtered",
                Message::Open(Id::Firewall),
            ));
        }
        let synced = facts.last_sync.as_deref().unwrap_or("an unknown time ago");
        rows.push(match facts.updates {
            0 => ui.link(
                Icon::Package,
                Tone::Mute,
                "Packages are up to date",
                format!("Last checked {synced}"),
                Message::Open(Id::Updates),
            ),
            count => ui.link(
                Icon::Package,
                Tone::Accent,
                format!("{count} {} to install", if count == 1 { "update" } else { "updates" }),
                format!("Last checked {synced}"),
                Message::Open(Id::Updates),
            ),
        });
        if snapshot.scheduler.running.is_none() && !snapshot.scheduler.available.is_empty() {
            rows.push(ui.link(
                Icon::Chip,
                Tone::Mute,
                "No sched-ext scheduler is running",
                format!("{} are installed", snapshot.scheduler.available.len()),
                Message::Open(Id::Scheduler),
            ));
        }
        ui.group("Worth a look", rows)
    }

    fn results<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        let matches = Id::search(&self.query);
        if matches.is_empty() {
            return ui.empty(
                Icon::Search,
                "Nothing matches",
                "Try a shorter word, or the name of a tool such as pacman or ufw.",
                None,
            );
        }
        let rows = matches
            .into_iter()
            .map(|id| ui.link(id.icon(), Tone::Accent, id.title(), id.section().title(), Message::Open(id)))
            .collect();
        ui.group("", rows)
    }

    /// Every panel by section, as a map of the program.
    fn index<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        let sections: Vec<_> =
            Section::ordered(ui.config).into_iter().filter(|s| *s != Section::Home).collect();
        let entry = |id: Id| {
            button(ui.text(id.title()).size(ui.metrics().text_size - 1.0))
                .padding([ui.d(4.0), 0.0])
                .style(style::ghost)
                .on_press(Message::Open(id))
                .into()
        };
        let list = |section: Section, columns: usize| -> Element<'a, Message> {
            let ids: Vec<Id> = section.panels(ui.config).collect();
            let rows = ids.len().div_ceil(columns).max(1);
            let lists = ids
                .chunks(rows)
                .map(|chunk| column(chunk.iter().map(|id| entry(*id))).into());
            column![ui.bold(section.title()), row(lists).spacing(18)].spacing(6).into()
        };
        // Sections differ a lot in length. Giving them all the same width would
        // leave the short ones half empty and wrap the long ones.
        let content: Element<'a, Message> = if ui.content > 1180.0 {
            row(sections.into_iter().map(|section| {
                let share = match section.panels(ui.config).count() {
                    0..=4 => 8,
                    5..=6 => 10,
                    _ => 13,
                };
                container(list(section, 2)).width(Length::FillPortion(share)).into()
            }))
            .spacing(24)
            .into()
        } else {
            let items = sections.into_iter().map(|section| list(section, 1)).collect();
            ui.grid(168.0, ui.content, 24.0, items)
        };
        column![ui.divider(), space().height(20), content].into()
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::batch([
            Task::perform(snapshot(), |s| Message::Loaded(Box::new(s))),
            Task::perform(facts(), |f| Message::Facts(Box::new(f))),
            sample(),
        ])
    }

    /// Runs every second, so it takes only the readings that cost nothing.
    fn refresh(&self) -> Task<Message> {
        sample()
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(snapshot) => {
                self.snapshot = Some(*snapshot);
                Effect::none()
            }
            Message::Facts(facts) => {
                self.facts = Some(*facts);
                Effect::none()
            }
            Message::Sampled(reading) => {
                let (sample, battery) = *reading;
                if self.sample.memory_total > 0 {
                    self.load = sample.cpu.load_since(self.sample.cpu);
                }
                if self.clock.len() == HISTORY {
                    self.clock.pop_front();
                }
                self.clock.push_back(sample.ghz);
                self.sample = sample;
                self.battery = battery;
                self.ticks += 1;
                if self.ticks.is_multiple_of(REFRESH) && self.dragging.is_none() {
                    return Effect::task(Task::perform(snapshot(), |s| Message::Loaded(Box::new(s))));
                }
                Effect::none()
            }
            Message::Search(query) => {
                self.query = query;
                Effect::none()
            }
            Message::OpenFirst => match Id::search(&self.query).first() {
                Some(id) => self.update(Message::Open(*id)),
                None => Effect::none(),
            },
            Message::Open(id) => {
                self.query.clear();
                Effect::request(Request::Open(id))
            }
            Message::Press(tile) => self.press(tile),
            Message::Drag(level, percent) => {
                self.dragging = Some((level, percent));
                match self.throttle.push((level, percent)) {
                    Some(_) => match self.command(level, percent) {
                        Some(command) => Effect::task(quietly(command, Message::Sent)),
                        None => Effect::none(),
                    },
                    None => Effect::none(),
                }
            }
            Message::Sent => match self.throttle.done() {
                Some((level, percent)) => match self.command(level, percent) {
                    Some(command) => Effect::task(quietly(command, Message::Sent)),
                    None => Effect::none(),
                },
                None => Effect::none(),
            },
            Message::Release(level) => {
                let Some((_, percent)) = self.dragging else { return Effect::none() };
                self.throttle.clear();
                let apply = self.command(level, percent);
                // The snapshot still holds the value from before the drag.
                self.dragging = None;
                let before = self.level(level);
                let undo = before.and_then(|before| self.command(level, before));
                if let Some(snapshot) = &mut self.snapshot {
                    snapshot.store(level, percent);
                }
                match (apply, undo) {
                    (Some(apply), Some(undo)) => {
                        Effect::change(sys::Change::new(apply).undo(undo))
                    }
                    _ => Effect::none(),
                }
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let search = ui.search(
            "Search every setting",
            "Ctrl K",
            |field| field.id(SEARCH).on_submit(Message::OpenFirst),
            &self.query,
            Message::Search,
        );

        let (Some(snapshot), Some(facts)) = (&self.snapshot, &self.facts) else {
            return ui.panel(Id::Home.title(), "", Some(search), vec![ui.loading()]);
        };
        let m = &facts.machine;
        let subtitle = [m.os.as_str(), &m.kernel, &m.cpu]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        let subtitle = format!("{subtitle}, {} memory", sys::bytes(self.sample.memory_total));
        let title = Id::Home.title();

        if !self.query.trim().is_empty() {
            return ui.panel(title, subtitle, Some(search), vec![self.results(ui)]);
        }

        let side_by_side = ui.content > 1100.0;
        let gap = ui.metrics().gap;
        let main_width = if side_by_side { ui.content - 404.0 - gap } else { ui.content };
        let tiles: Vec<_> = ui
            .config
            .tiles
            .iter()
            .filter_map(|key| Tile::ALL.into_iter().find(|tile| tile.key() == key))
            .map(|tile| self.tile(ui, tile))
            .collect();

        let memory = ui.card(ui.meter(
            "Memory",
            format!(
                "{} of {}",
                sys::bytes(self.sample.memory_used),
                sys::bytes(self.sample.memory_total)
            ),
            self.sample.memory_used as f32 / self.sample.memory_total.max(1) as f32,
        ));
        let mut main = vec![
            ui.grid(172.0, main_width, 12.0, tiles),
            ui.columns(self.levels(ui), self.processor(ui, facts)),
        ];
        main.push(match &facts.root {
            Some(root) => {
                let shown = format!("{} of {}", sys::bytes(root.used), sys::bytes(root.size));
                ui.columns(memory, ui.card(ui.meter("Storage", shown, root.fraction())))
            }
            None => memory,
        });

        let mut side = Vec::new();
        if let Some(battery) = &self.battery {
            side.push(battery_card(ui, battery));
        }
        side.push(self.notes(ui, snapshot, facts));

        let body: Element<'a, Message> = if side_by_side {
            row![
                container(ui.stack(main)).width(Fill),
                container(ui.stack(side)).width(404),
            ]
            .spacing(gap)
            .into()
        } else {
            ui.stack(main.into_iter().chain(side).collect())
        };
        ui.panel(title, subtitle, Some(search), vec![body, self.index(ui)])
    }

    fn watch(&self) -> Watch {
        Watch::Every(Duration::from_secs(1))
    }
}

impl Snapshot {
    fn store(&mut self, level: Level, percent: f32) {
        fn default(devices: &mut [audio::Device]) -> Option<&mut audio::Device> {
            devices.iter_mut().find(|device| device.default)
        }
        match (level, &mut self.audio) {
            (Level::Brightness, _) => self.brightness = Some(percent),
            (Level::Speaker, Some(audio)) => {
                if let Some(device) = default(&mut audio.outputs) {
                    device.volume = percent;
                }
            }
            (Level::Microphone, Some(audio)) => {
                if let Some(device) = default(&mut audio.inputs) {
                    device.volume = percent;
                }
            }
            (_, None) => {}
        }
    }
}

fn sample() -> Task<Message> {
    Task::future(async {
        Message::Sampled(Box::new((probe::sample(), power::battery())))
    })
}

/// The battery card, shared with the power panel.
pub fn battery_card<'a, M: 'a>(ui: Ui<'a>, battery: &power::Battery) -> Element<'a, M> {
    let time = battery.hours().map(|_| battery.summary()).unwrap_or_else(|| battery.summary());
    let (headline, detail) = match time.split_once(", ") {
        Some((headline, detail)) => {
            let mut detail = detail.to_owned();
            if let Some(first) = detail.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            (headline.to_owned(), detail)
        }
        None => (time, String::new()),
    };
    let header = row![
        container(ui.percentage(battery.percent)).width(Fill),
        column![ui.bold(headline), ui.small(detail)].align_x(iced::Right),
    ]
    .align_y(iced::Bottom);
    ui.card(column![header, gauge(ui, battery)].spacing(14))
}

/// The gauge with its scale.
pub fn gauge<'a, M: 'a>(ui: Ui<'a>, battery: &power::Battery) -> Element<'a, M> {
    let mark = |value: f32, label: &'static str, align: iced::Alignment| {
        column![ui.bold(format!("{value:.1} Wh")), ui.small(label)].align_x(align)
    };
    let start = iced::Alignment::Start;
    let end = iced::Alignment::End;
    let health = battery.health();
    let now = mark(battery.now, "now", start);
    let full = mark(battery.full, "full today", end);
    let new = mark(battery.design, "when new", end);
    // Labels sit under the marks they name, unless wear is so small or so large
    // that they would collide.
    let scale: Element<'a, M> = if (0.4..0.82).contains(&health) {
        let share = (health * 100.0) as u16;
        row![
            row![now, space().width(Fill), full]
                .padding(iced::Padding::ZERO.right(6))
                .width(Length::FillPortion(share)),
            container(new).align_right(Length::FillPortion(100 - share)),
        ]
        .into()
    } else {
        row![now, space().width(Fill), full, space().width(Fill), new].into()
    };
    let gauge = chart::Gauge { now: battery.now, full: battery.full, design: battery.design };
    column![gauge.view(), scale].spacing(8).into()
}
