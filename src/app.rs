//! The application shell: navigation, the receipt bar and the theme.

use std::path::PathBuf;
use std::time::Duration;

use iced::futures::{SinkExt, Stream};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, operation, responsive, row, scrollable};
use iced::{Center, Fill, Size, Subscription, Task, keyboard, stream, window};

use crate::config::Config;
use crate::icon::Icon;
use crate::panels::{self, Effect, Id, Panels, Request, Section, Watch, home};
use crate::sys::{self, Op, desktop};
use crate::theme::style::{self, Tone};
use crate::theme::{Library, Theme, library};
use crate::ui::{Element, Kind, Ui, Width};

pub struct App {
    config: Config,
    library: Library,
    theme: Theme,
    desktop_dark: bool,
    panels: Panels,
    active: Id,
    /// The panel last open in each section, so the rail returns to it.
    visited: Vec<(Section, Id)>,
    receipt: Option<Receipt>,
    focused: bool,
    /// A theme is being edited, which a reload must not interrupt.
    previewing: bool,
}

const STRIP: &str = "strip";

/// The last change, as shown at the bottom of the window.
struct Receipt {
    text: String,
    state: Progress,
    undo: Option<Op>,
    panel: Id,
}

enum Progress {
    Running,
    Done,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    Panel(panels::Message),
    Open(Id),
    OpenSection(Section),
    Refresh(Id),
    Applied(Id, sys::Result<()>),
    Undo,
    Dismiss,
    Key(keyboard::Event),
    Focused(bool),
    ThemesChanged,
    Desktop(bool),
}

impl App {
    /// Starts on `panel`, or on the one chosen in the preferences.
    pub fn new(panel: Option<Id>) -> (Self, Task<Message>) {
        let config = Config::load();
        let library = Library::load(&library::dir());
        let active = panel.or_else(|| Id::from_key(&config.start)).unwrap_or(Id::Home);
        let theme = Theme::new(library.get_or_default(config.active_theme(true)).clone());
        let app = Self {
            config,
            library,
            theme,
            desktop_dark: true,
            panels: Panels::default(),
            active,
            visited: Vec::new(),
            receipt: None,
            // The compositor says so once it is. A window that opens on another
            // workspace must not start polling before anyone looks at it.
            focused: false,
            previewing: false,
        };
        let load = Task::batch([
            app.load_section(active),
            app.reveal(active),
            Task::perform(desktop::look(), |look| Message::Desktop(look.dark)),
        ]);
        (app, load)
    }

    pub fn title(&self) -> String {
        format!("{} - HyprKontrol", self.active.title())
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    /// Loads a panel first, then its neighbours for their sidebar summaries.
    fn load_section(&self, active: Id) -> Task<Message> {
        let load = |id: Id| self.panels.load(id).map(Message::Panel);
        let neighbours = active.section().panels(&self.config).filter(|id| *id != active).map(load);
        Task::batch(std::iter::once(load(active)).chain(neighbours))
    }

    fn open(&mut self, id: Id) -> Task<Message> {
        let section = id.section();
        let moved = section != self.active.section();
        self.active = id;
        self.visited.retain(|(known, _)| *known != section);
        self.visited.push((section, id));
        let load =
            if moved { self.load_section(id) } else { self.panels.load(id).map(Message::Panel) };
        Task::batch([self.reveal(id), load])
    }

    /// Scrolls the strip so that `id` is visible. Without this, a panel opened
    /// from search could be selected somewhere out of sight.
    fn reveal(&self, id: Id) -> Task<Message> {
        let panels: Vec<_> = id.section().panels(&self.config).collect();
        let last = panels.len().saturating_sub(1).max(1);
        let position = panels.iter().position(|known| *known == id).unwrap_or(0);
        let x = position as f32 / last as f32;
        operation::snap_to(STRIP, scrollable::RelativeOffset { x, y: 0.0 })
    }

    fn apply_theme(&mut self) {
        let id = self.config.active_theme(self.desktop_dark);
        self.theme = Theme::new(self.library.get_or_default(id).clone());
        self.previewing = false;
    }

    fn reload_themes(&mut self) {
        self.library = Library::load(&library::dir());
        self.apply_theme();
    }

    fn run(&mut self, panel: Id, op: Op, undo: Option<Op>) -> Task<Message> {
        self.receipt = Some(Receipt { text: op.describe(), state: Progress::Running, undo, panel });
        Task::perform(op.apply(), move |result| Message::Applied(panel, result))
    }

    fn settle(&mut self, panel: Id, effect: Effect<panels::Message>) -> Task<Message> {
        let mut tasks = vec![effect.task.map(Message::Panel)];
        if let Some(change) = effect.change {
            tasks.push(self.run(panel, change.apply, change.undo));
        }
        match effect.request {
            None => {}
            Some(Request::Open(id)) => tasks.push(self.open(id)),
            Some(Request::Configure(config)) => {
                self.config = *config;
                if let Err(error) = self.config.save() {
                    self.report(panel, error.to_string());
                }
                self.apply_theme();
            }
            Some(Request::PreviewTheme(spec)) => {
                self.theme = Theme::new(*spec);
                self.previewing = true;
            }
            Some(Request::ReloadThemes) => self.reload_themes(),
            Some(Request::Report(error)) => self.report(panel, error.to_string()),
            Some(Request::Done(text)) => {
                self.receipt = Some(Receipt { text, state: Progress::Done, undo: None, panel });
            }
        }
        Task::batch(tasks)
    }

    fn report(&mut self, panel: Id, reason: String) {
        self.receipt = Some(Receipt {
            text: String::new(),
            state: Progress::Failed(reason),
            undo: None,
            panel,
        });
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Panel(message) => {
                let id = message.id();
                let effect = self.panels.update(message);
                self.settle(id, effect)
            }
            Message::Open(id) => self.open(id),
            Message::OpenSection(section) => {
                let last = self.visited.iter().find(|(known, _)| *known == section);
                let first = section.panels(&self.config).next();
                match last.map(|(_, id)| *id).or(first) {
                    Some(id) => self.open(id),
                    None => Task::none(),
                }
            }
            Message::Refresh(id) => self.panels.refresh(id).map(Message::Panel),
            Message::Applied(panel, result) => {
                if let Some(receipt) = &mut self.receipt {
                    receipt.state = match result {
                        Ok(()) => Progress::Done,
                        Err(error) => Progress::Failed(error.to_string()),
                    };
                }
                // Reload on failure too, so that a switch that did not take
                // effect goes back to where it was.
                let reload = self.panels.load(panel).map(Message::Panel);
                let look = Task::perform(desktop::look(), |look| Message::Desktop(look.dark));
                if self.config.follow_desktop { Task::batch([reload, look]) } else { reload }
            }
            Message::Undo => {
                let Some(receipt) = &mut self.receipt else { return Task::none() };
                let panel = receipt.panel;
                match receipt.undo.take() {
                    Some(undo) => self.run(panel, undo, None),
                    None => Task::none(),
                }
            }
            Message::Dismiss => {
                self.receipt = None;
                Task::none()
            }
            Message::Key(event) => self.key(event),
            Message::Focused(focused) => {
                self.focused = focused;
                // Whatever changed in the meantime was not watched.
                if focused {
                    self.panels.load(self.active).map(Message::Panel)
                } else {
                    Task::none()
                }
            }
            Message::ThemesChanged => {
                if !self.previewing {
                    self.reload_themes();
                }
                Task::none()
            }
            Message::Desktop(dark) => {
                let changed = self.desktop_dark != dark;
                self.desktop_dark = dark;
                if changed && self.config.follow_desktop && !self.previewing {
                    self.apply_theme();
                }
                Task::none()
            }
        }
    }

    fn key(&mut self, event: keyboard::Event) -> Task<Message> {
        use keyboard::key::{Key, Named};

        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return Task::none();
        };
        match key.as_ref() {
            Key::Character("k" | "f") if modifiers.command() => {
                let open = self.open(Id::Home);
                Task::batch([open, operation::focus(home::SEARCH)])
            }
            Key::Named(Named::Tab) if modifiers.is_empty() => operation::focus_next(),
            Key::Named(Named::Tab) if modifiers == keyboard::Modifiers::SHIFT => {
                operation::focus_previous()
            }
            // Alt and a number open a section, in the order of the rail.
            Key::Character(digit) if modifiers.alt() => {
                let sections = Section::ordered(&self.config);
                let section = digit
                    .parse::<usize>()
                    .ok()
                    .and_then(|number| sections.get(number.checked_sub(1)?).copied());
                match section {
                    Some(section) => self.update(Message::OpenSection(section)),
                    None => Task::none(),
                }
            }
            _ => Task::none(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let events = iced::event::listen_with(|event, _, _| match event {
            iced::Event::Window(window::Event::Focused) => Some(Message::Focused(true)),
            iced::Event::Window(window::Event::Unfocused) => Some(Message::Focused(false)),
            _ => None,
        });
        let keys = keyboard::listen().map(Message::Key);
        let themes = Subscription::run(watch_themes);
        // A settings window is often left open behind others. It should cost
        // nothing there.
        let watch = match self.panels.watch(self.active) {
            _ if !self.focused => Subscription::none(),
            Watch::Never => Subscription::none(),
            Watch::Every(period) => {
                iced::time::every(period).with(self.active).map(|(id, _)| Message::Refresh(id))
            }
            Watch::Command(command) => Subscription::run_with(command, watch_command)
                .with(self.active)
                .map(|(id, ())| Message::Refresh(id)),
        };
        Subscription::batch([events, keys, themes, watch])
    }

    pub fn view(&self) -> Element<'_, Message> {
        responsive(|size| self.shell(size)).into()
    }

    /// Lays the window out for its size. Public so that it can be rendered
    /// without a window.
    pub fn shell(&self, size: Size) -> Element<'_, Message> {
        let width = Width::of(size.width);
        let m = self.theme.metrics();
        let alone = self.active.section() == Section::Home;
        let rail = if width == Width::Narrow { 0.0 } else { m.rail_width };
        let sidebar = if width == Width::Wide && !alone { m.sidebar_width } else { 0.0 };
        let content = size.width - rail - sidebar - 2.0 * Ui::side_padding(m, width);
        let ui =
            Ui { theme: &self.theme, config: &self.config, library: &self.library, width, content };

        let mut page =
            column![self.panels.view(self.active, ui).map(Message::Panel)].width(Fill).height(Fill);
        if let Some(receipt) = &self.receipt
            && (self.config.receipt || matches!(receipt.state, Progress::Failed(_)))
        {
            page = page.push(self.receipt(ui, receipt));
        }
        let page = container(page).style(style::content).width(Fill).height(Fill);

        match width {
            Width::Wide if alone => row![self.rail(ui), page].into(),
            Width::Wide => row![self.rail(ui), self.sidebar(ui), page].into(),
            Width::Medium if alone => row![self.rail(ui), page].into(),
            Width::Medium => row![self.rail(ui), column![self.strip(ui), page]].into(),
            Width::Narrow if alone => column![page, self.rail(ui)].into(),
            Width::Narrow => column![self.strip(ui), page, self.rail(ui)].into(),
        }
    }

    /// Sections. A column on the left, or a bar at the bottom when narrow.
    fn rail<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let narrow = ui.width == Width::Narrow;
        let labels = self.theme.spec().rail_labels;
        let items = Section::ordered(&self.config).into_iter().map(|section| {
            let on = section == self.active.section();
            let tone = if on { Tone::Accent } else { Tone::Mute };
            let mut content = column![ui.icon(section.icon(), tone)].spacing(5).align_x(Center);
            if labels {
                content = content.push(
                    ui.strong(section.title())
                        .size(ui.metrics().text_size - 3.0)
                        .style(style::ink(tone))
                        .wrapping(Wrapping::None),
                );
            }
            let item = button(container(content).center_x(Fill).center_y(Fill))
                .padding(0)
                .style(style::rail_item(on))
                .on_press(Message::OpenSection(section));
            if narrow {
                item.width(Fill).height(54).into()
            } else {
                item.width(Fill).height(ui.d(58.0)).into()
            }
        });
        if narrow {
            container(row(items).spacing(2)).padding([6, 8]).style(style::rail).width(Fill).into()
        } else {
            container(column(items).spacing(4))
                .padding([14, 10])
                .style(style::rail)
                .width(ui.metrics().rail_width)
                .height(Fill)
                .into()
        }
    }

    /// Panels of the section with their current state.
    fn sidebar<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let section = self.active.section();
        let items = section.panels(&self.config).map(|id| {
            let on = id == self.active;
            let tone = if on { Tone::Accent } else { Tone::Mute };
            let mut labels = column![ui.strong(id.title()).wrapping(Wrapping::None)].spacing(1);
            let summary = self.panels.summary(id);
            if !summary.is_empty() {
                let tone = if on { Tone::Dim } else { Tone::Mute };
                labels =
                    labels.push(ui.small(summary).style(style::ink(tone)).wrapping(Wrapping::None));
            }
            let labels = container(labels).width(Fill).clip(true);
            button(row![ui.icon(id.icon(), tone), labels].spacing(14).align_y(Center))
                .padding([ui.d(10.0), 12.0])
                .width(Fill)
                .style(style::nav(on))
                .on_press(Message::Open(id))
                .into()
        });
        let heading = container(ui.heading(section.title())).padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            bottom: 14.0,
            left: 12.0,
        });
        let content = column![heading, column(items).spacing(2)].padding([22, 14]);
        container(scrollable(content).height(Fill))
            .style(style::sidebar)
            .width(ui.metrics().sidebar_width)
            .height(Fill)
            .into()
    }

    /// The sidebar for windows without room for one: a row that scrolls.
    fn strip<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let items = self.active.section().panels(&self.config).map(|id| {
            let on = id == self.active;
            let tone = if on { Tone::Accent } else { Tone::Mute };
            let label = ui.strong(id.title()).wrapping(Wrapping::None);
            button(row![ui.icon(id.icon(), tone), label].spacing(10).align_y(Center))
                .padding([ui.d(11.0), 14.0])
                .style(style::nav(on))
                .on_press(Message::Open(id))
                .into()
        });
        let bar = scrollable::Scrollbar::new().width(0).scroller_width(0);
        let strip = scrollable(row(items).spacing(4).padding([10, 14]))
            .id(STRIP)
            .direction(scrollable::Direction::Horizontal(bar))
            .width(Fill);
        container(strip).style(style::sidebar).width(Fill).into()
    }

    fn receipt<'a>(&'a self, ui: Ui<'a>, receipt: &'a Receipt) -> Element<'a, Message> {
        let (label, text, tone) = match &receipt.state {
            Progress::Running => ("Running", receipt.text.as_str(), Tone::Dim),
            Progress::Done => ("Last change", receipt.text.as_str(), Tone::Live),
            Progress::Failed(reason) => ("Not changed", reason.as_str(), Tone::Critical),
        };
        // A command can be cut short, its start says enough. The end of an error
        // is often the part that matters, so errors get all the lines they need.
        let wrapping = match &receipt.state {
            Progress::Failed(_) => Wrapping::WordOrGlyph,
            _ => Wrapping::None,
        };
        let text = container(ui.mono(text).style(style::ink(tone)).wrapping(wrapping))
            .width(Fill)
            .clip(true);
        let mut content = row![].spacing(14).align_y(Center);
        if ui.width != Width::Narrow {
            content = content.push(ui.small(label));
        }
        content = content.push(text);
        if receipt.undo.is_some() && matches!(receipt.state, Progress::Done) {
            content = content.push(ui.button("Undo", Kind::Link, Some(Message::Undo)));
        }
        content = content.push(
            button(ui.sized_icon(Icon::Close, Tone::Mute, 16.0))
                .padding(8)
                .style(style::ghost)
                .on_press(Message::Dismiss),
        );
        let side = Ui::side_padding(ui.metrics(), ui.width);
        let bar = container(content).padding([ui.d(8.0), side]).width(Fill).align_y(Center);
        column![ui.divider(), bar].into()
    }
}

/// Signals whenever a theme file is added, edited or removed.
///
/// Modification times are compared on a timer. That works the same for editors
/// that write in place and those that replace the file, and for themes behind
/// symlinks, all of which need special care with inotify.
fn watch_themes() -> impl Stream<Item = Message> {
    type Stamp = Vec<(PathBuf, Option<std::time::SystemTime>)>;

    fn stamp() -> Stamp {
        let mut stamp: Stamp = library::files(&library::dir())
            .into_iter()
            .map(|path| {
                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                (path, modified)
            })
            .collect();
        stamp.sort();
        stamp
    }

    stream::channel(1, async |mut output| {
        let mut known = stamp();
        loop {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let current = stamp();
            if current != known {
                known = current;
                let _ = output.send(Message::ThemesChanged).await;
            }
        }
    })
}

/// Runs a monitoring command and signals when it reports something.
///
/// These tools print bursts of lines for a single event, so a signal is held
/// back until the burst is over.
fn watch_command(command: &&'static [&'static str]) -> impl Stream<Item = ()> + use<> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let command = *command;
    stream::channel(1, async move |mut output| {
        let [program, args @ ..] = command else { return };
        let child = tokio::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let Ok(mut child) = child else { return };
        let Some(stdout) = child.stdout.take() else { return };
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(_)) = lines.next_line().await {
            let quiet = Duration::from_millis(250);
            while let Ok(Ok(Some(_))) = tokio::time::timeout(quiet, lines.next_line()).await {}
            let _ = output.send(()).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::StreamExt;
    use iced_test::Simulator;
    use iced_test::runtime::{Action, task};

    const SIZES: [(&str, Size); 3] = [
        ("wide", Size::new(1516.0, 800.0)),
        ("half", Size::new(752.0, 800.0)),
        ("narrow", Size::new(400.0, 800.0)),
    ];

    fn app() -> App {
        let library = Library::load(std::path::Path::new(""));
        App {
            config: Config::default(),
            theme: Theme::new(library.get_or_default(library::DEFAULT).clone()),
            library,
            desktop_dark: true,
            panels: Panels::default(),
            active: Id::Home,
            visited: Vec::new(),
            receipt: None,
            focused: true,
            previewing: false,
        }
    }

    /// Runs a task to completion and feeds its messages back, as the runtime would.
    async fn drive(app: &mut App, task: Task<Message>) {
        let mut pending = vec![task];
        while let Some(task) = pending.pop() {
            let Some(mut actions) = task::into_stream(task) else { continue };
            while let Some(action) = actions.next().await {
                if let Action::Output(message) = action {
                    pending.push(app.update(message));
                }
            }
        }
    }

    fn simulate(app: &App, size: Size) -> Simulator<'_, Message, Theme> {
        let settings = iced::Settings {
            default_font: app.theme.font(iced::font::Weight::Normal),
            ..iced::Settings::default()
        };
        Simulator::with_size(settings, size, app.shell(size))
    }

    #[test]
    fn every_panel_renders_at_every_size_before_it_has_loaded() {
        let mut app = app();
        for id in Id::ALL {
            app.active = *id;
            for (_, size) in SIZES {
                simulate(&app, size).snapshot(&app.theme).unwrap();
            }
        }
    }

    #[test]
    fn navigation_remembers_the_panel_of_each_section() {
        let mut app = app();
        let _ = app.update(Message::Open(Id::Sound));
        let _ = app.update(Message::Open(Id::Firewall));
        let _ = app.update(Message::OpenSection(Section::Devices));
        assert_eq!(app.active, Id::Sound);
        let _ = app.update(Message::OpenSection(Section::Desktop));
        assert_eq!(app.active, Section::Desktop.panels(&app.config).next().unwrap());
    }

    #[test]
    fn undo_is_offered_once() {
        let mut app = app();
        let write = |value: &str| Op::from(sys::Cmd::new("true").arg(value));
        let _ = app.run(Id::Power, write("new"), Some(write("old")));
        let _ = app.update(Message::Applied(Id::Power, Ok(())));
        let _ = app.update(Message::Undo);
        let receipt = app.receipt.as_ref().unwrap();
        assert_eq!(receipt.text, "true old");
        assert!(receipt.undo.is_none());
    }

    #[test]
    fn failures_are_shown_with_their_reason() {
        let mut app = app();
        let _ = app.run(Id::Power, sys::Cmd::new("false").into(), None);
        let _ = app.update(Message::Applied(Id::Power, Err(sys::Error::new("No permission"))));
        let mut ui = simulate(&app, SIZES[0].1);
        assert!(ui.find("No permission").is_ok());
    }

    /// Renders panels with data from the running system. Reads only.
    ///
    ///     SHOTS=target/shots cargo test screenshots -- --ignored
    ///
    /// `PANELS` limits the panels, `THEME` picks the theme and `SIZES` the window
    /// sizes; lists are separated by commas. `EDIT` opens the theme editor.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs a running desktop"]
    async fn screenshots() {
        let list = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|text| text.split(',').map(str::to_owned).collect::<Vec<_>>())
        };
        let dir = PathBuf::from(std::env::var("SHOTS").unwrap_or("target/shots".into()));
        let (panels, sizes) = (list("PANELS"), list("SIZES"));
        std::fs::create_dir_all(&dir).unwrap();

        for id in Id::ALL {
            if panels.as_ref().is_some_and(|keys| !keys.iter().any(|key| key == id.key())) {
                continue;
            }
            let mut app = app();
            if let Ok(theme) = std::env::var("THEME") {
                app.theme = Theme::new(app.library.get(&theme).unwrap().clone());
            }
            app.active = *id;
            let load = app.load_section(*id);
            drive(&mut app, load).await;
            if let Watch::Every(_) = app.panels.watch(*id) {
                for _ in 0..if *id == Id::Home { 40 } else { 2 } {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    let refresh = app.panels.refresh(*id).map(Message::Panel);
                    drive(&mut app, refresh).await;
                }
            }
            if *id == Id::Theme && std::env::var_os("EDIT").is_some() {
                let mut spec = app.theme.spec().clone();
                spec.name = format!("My {}", spec.name);
                for message in
                    [panels::theme::Message::Edit(Box::new(spec)), panels::theme::Message::Role(11)]
                {
                    let task = app.update(Message::Panel(panels::Message::Theme(message)));
                    drive(&mut app, task).await;
                }
            }
            for (name, size) in SIZES {
                if sizes.as_ref().is_some_and(|keys| !keys.iter().any(|key| key == name)) {
                    continue;
                }
                let snapshot = simulate(&app, size).snapshot(&app.theme).unwrap();
                let path = dir.join(format!("{}-{name}", id.key()));
                let file = dir.join(format!("{}-{name}-tiny-skia.png", id.key()));
                let _ = std::fs::remove_file(file);
                snapshot.matches_image(path).unwrap();
            }
        }
    }
}
