use std::path::PathBuf;

use iced::widget::{button, column, container, row, space};
use iced::{Center, Color, Fill, Task};

use super::home::Tile;
use super::{Effect, Id, Panel, Request, Section};
use crate::config::Config;
use crate::icon::Icon;
use crate::sys::{self, Cmd};
use crate::theme::color::READABLE;
use crate::theme::library::{self, Library};
use crate::theme::spec::{KNOBS, ROLES};
use crate::theme::{Spec, Tone, style};
use crate::ui::color::{ColorEdit, ColorMessage};
use crate::ui::{Element, Kind, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Themes,
    Layout,
}

#[derive(Default)]
pub struct State {
    tab: Tab,
    /// Theme being edited. Nothing is written until it is saved.
    draft: Option<Spec>,
    role: Option<usize>,
    color: ColorEdit,
    import: String,
    confirm_delete: bool,
    fonts: (Vec<String>, Vec<String>),
}

#[derive(Debug, Clone)]
pub enum Message {
    Fonts(Vec<String>, Vec<String>),
    Tab(Tab),
    Use(Box<Config>),
    Edit(Box<Spec>),
    Role(usize),
    Color(ColorMessage),
    Knob(usize, f32),
    RailLabels(bool),
    Name(String),
    Author(String),
    UiFont(Font),
    MonoFont(Font),
    Save,
    Discard,
    AskDelete(bool),
    Delete(PathBuf),
    OpenFolder,
    ImportPath(String),
    Import,
    Configure(Box<Config>),
    Settled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font(Option<String>);

impl std::fmt::Display for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_deref().unwrap_or("Desktop font"))
    }
}

/// Moves `index` one step towards the front or the back.
fn shift<T>(list: &mut [T], index: usize, forward: bool) {
    if forward && index + 1 < list.len() {
        list.swap(index, index + 1);
    } else if !forward && index > 0 {
        list.swap(index, index - 1);
    }
}

impl State {
    fn preview(&self) -> Effect<Message> {
        match &self.draft {
            Some(draft) => Effect::request(Request::PreviewTheme(Box::new(draft.clone()))),
            None => Effect::none(),
        }
    }

    fn change(&mut self, edit: impl FnOnce(&mut Spec)) -> Effect<Message> {
        if let Some(draft) = &mut self.draft {
            edit(draft);
        }
        self.preview()
    }

    /// A card that shows a theme in its own colours.
    fn swatch_card<'a>(&self, ui: Ui<'a>, spec: &'a Spec) -> Element<'a, Message> {
        let selected = ui.theme.spec().id == spec.id;
        let c = spec.colors;
        let opaque = |color: Color| Color { a: 1.0, ..color };
        let block = |color: Color, width: f32, height: f32, radius: f32| {
            container(space()).width(width).height(height).style(move |_| container::Style {
                background: Some(color.into()),
                border: iced::border::rounded(radius),
                ..container::Style::default()
            })
        };
        let radius = spec.metrics.card_radius.min(12.0) * 0.6;
        let sample = row![
            block(opaque(c.rail), 18.0, 84.0, 0.0),
            container(
                column![
                    block(c.text, 64.0, 6.0, 3.0),
                    row![
                        block(c.accent, 34.0, 22.0, radius),
                        block(opaque(c.card), 34.0, 22.0, radius),
                        block(opaque(c.raised), 34.0, 22.0, radius),
                    ]
                    .spacing(6),
                    block(c.live, 90.0, 4.0, 2.0),
                ]
                .spacing(9)
            )
            .padding(12)
            .width(Fill)
            .height(84)
            .style(move |_| container::Style {
                background: Some(opaque(c.background).into()),
                ..container::Style::default()
            }),
        ];
        let label = row![
            column![
                ui.bold(spec.name.as_str()),
                ui.small(if spec.path.is_some() { "Yours" } else { "Built in" }),
            ]
            .width(Fill),
            if selected { ui.icon(Icon::Check, Tone::Accent) } else { space().into() },
        ]
        .align_y(Center)
        .padding([10, 12]);
        let mut config = ui.config.clone();
        config.follow_desktop = false;
        config.theme.clone_from(&spec.id);
        button(column![sample, label])
            .padding(3)
            .width(Fill)
            .clip(true)
            .style(style::thumbnail(selected))
            .on_press(Message::Use(Box::new(config)))
            .into()
    }

    fn gallery<'a>(&'a self, ui: Ui<'a>) -> Vec<Element<'a, Message>> {
        let cards = ui.library.themes.iter().map(|spec| self.swatch_card(ui, spec)).collect();
        let current = ui.theme.spec();
        let mut body = vec![ui.grid(196.0, ui.content, 14.0, cards)];

        for (file, reason) in &ui.library.errors {
            body.push(ui.failure(format!("{file} could not be loaded: {reason}")));
        }

        let edit = if current.path.is_some() { "Edit" } else { "Copy and edit" };
        let mut controls =
            vec![ui.button(edit, Kind::Primary, Some(Message::Edit(Box::new(copy(ui)))))];
        match (&current.path, self.confirm_delete) {
            (Some(path), true) => controls.extend([
                ui.button(
                    "Delete for good",
                    Kind::Danger,
                    Some(Message::Delete(path.clone())),
                ),
                ui.button("Keep", Kind::Secondary, Some(Message::AskDelete(false))),
            ]),
            (Some(_), false) => controls.push(ui.button(
                "Delete",
                Kind::Danger,
                Some(Message::AskDelete(true)),
            )),
            (None, _) => {}
        }
        controls.push(ui.button("Open theme folder", Kind::Secondary, Some(Message::OpenFolder)));
        body.push(ui.group(
            "",
            vec![
                ui.row(
                    current.name.as_str(),
                    if current.author.is_empty() {
                        "No author".to_owned()
                    } else {
                        format!("By {}", current.author)
                    },
                    ui.actions(controls),
                ),
                ui.cell(
                    row![
                        ui.field(
                            "Path to a theme file from someone else",
                            &self.import,
                            Message::ImportPath
                        )
                        .on_submit(Message::Import),
                        ui.button(
                            "Import",
                            Kind::Secondary,
                            (!self.import.trim().is_empty()).then_some(Message::Import)
                        ),
                    ]
                    .spacing(8)
                    .align_y(Center),
                ),
            ],
        ));
        body.push(self.follow(ui));
        body
    }

    fn follow<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        let set = |edit: &dyn Fn(&mut Config)| {
            let mut config = ui.config.clone();
            edit(&mut config);
            Message::Configure(Box::new(config))
        };
        let on = ui.config.follow_desktop;
        let toggled = set(&|config| config.follow_desktop = !on);
        let mut rows = vec![ui.toggle(
            "Follow the desktop",
            "Switches theme when the desktop goes dark or light",
            on,
            Some(move |_| toggled.clone()),
        )];
        if on {
            let names: Vec<String> =
                ui.library.themes.iter().map(|spec| spec.name.clone()).collect();
            let find = |id: &str| ui.library.get(id).map(|spec| spec.name.clone());
            let pick = |selected: &str, apply: fn(&mut Config, String)| {
                let config = ui.config.clone();
                let themes = ui.library.themes.clone();
                ui.pick(names.clone(), find(selected), move |name| {
                    let mut config = config.clone();
                    if let Some(spec) = themes.iter().find(|spec| spec.name == name) {
                        apply(&mut config, spec.id.clone());
                    }
                    Message::Configure(Box::new(config))
                })
            };
            rows.push(ui.row(
                "While light",
                "",
                pick(&ui.config.light_theme, |config, id| config.light_theme = id),
            ));
            rows.push(ui.row(
                "While dark",
                "",
                pick(&ui.config.dark_theme, |config, id| config.dark_theme = id),
            ));
        }
        ui.group("", rows)
    }

    fn editor<'a>(&'a self, ui: Ui<'a>, draft: &'a Spec) -> Vec<Element<'a, Message>> {
        let identity = ui.group(
            "",
            vec![ui.cell(
                row![
                    ui.field("Name", &draft.name, Message::Name),
                    ui.field("Author", &draft.author, Message::Author),
                ]
                .spacing(10),
            )],
        );

        let mut roles = Vec::new();
        for (index, role) in ROLES.iter().enumerate() {
            let color = (role.get)(&draft.colors);
            let selected = self.role == Some(index);
            let content = row![
                ui.swatch(color, None),
                column![ui.text(role.label), ui.small(role.hint)].width(Fill),
                ui.mono(crate::theme::color::Hex(color).to_string())
                    .style(style::ink(Tone::Mute)),
            ]
            .spacing(14)
            .align_y(Center);
            let line = button(content)
                .padding([ui.d(8.0), 18.0])
                .width(Fill)
                .style(style::row)
                .on_press(Message::Role(index));
            roles.push(if selected {
                column![line, ui.cell(self.color.view(ui, true, Message::Color))].into()
            } else {
                line.into()
            });
        }

        let checks = draft
            .colors
            .readability()
            .map(|(label, ratio)| {
                let (icon, tone, verdict) = if ratio >= READABLE {
                    (Icon::Check, Tone::Live, "Readable")
                } else {
                    (Icon::Warning, Tone::Warning, "Hard to read, aim for 4.5")
                };
                ui.icon_row(icon, tone, label, verdict, ui.dim(format!("{ratio:.1}")))
            })
            .collect();

        let mut shape: Vec<_> = KNOBS
            .iter()
            .enumerate()
            .map(|(index, knob)| {
                let value = (knob.get)(&draft.metrics);
                let shown = if knob.step < 1.0 {
                    format!("{value:.2}")
                } else {
                    format!("{value:.0}")
                };
                ui.slide(
                    knob.label,
                    knob.min..=knob.max,
                    knob.step,
                    value,
                    shown,
                    move |value| Message::Knob(index, value),
                    Message::Settled,
                )
            })
            .collect();
        shape.push(ui.toggle(
            "Labels under rail icons",
            "",
            draft.rail_labels,
            Some(Message::RailLabels),
        ));

        let choices = |families: &[String]| -> Vec<Font> {
            std::iter::once(Font(None))
                .chain(families.iter().map(|family| Font(Some(family.clone()))))
                .collect()
        };
        let fonts = ui.group(
            "Fonts",
            vec![
                ui.row(
                    "Interface",
                    "",
                    ui.pick(
                        choices(&self.fonts.0),
                        Some(Font(draft.fonts.ui.clone())),
                        Message::UiFont,
                    ),
                ),
                ui.row(
                    "Commands",
                    "",
                    ui.pick(
                        choices(&self.fonts.1),
                        Some(Font(draft.fonts.mono.clone())),
                        Message::MonoFont,
                    ),
                ),
            ],
        );

        vec![
            identity,
            ui.columns(
                ui.group("Colours", roles),
                ui.stack(vec![
                    ui.group("Readability", checks),
                    ui.group("Shape and spacing", shape),
                    fonts,
                ]),
            ),
        ]
    }

    fn layout<'a>(&self, ui: Ui<'a>) -> Vec<Element<'a, Message>> {
        let set = |edit: &dyn Fn(&mut Config)| {
            let mut config = ui.config.clone();
            edit(&mut config);
            Message::Configure(Box::new(config))
        };
        let arrows = |index: usize, count: usize, moved: &dyn Fn(bool) -> Message| {
            ui.actions(vec![
                ui.button("Up", Kind::Secondary, (index > 0).then(|| moved(false))),
                ui.button("Down", Kind::Secondary, (index + 1 < count).then(|| moved(true))),
            ])
        };

        let receipt = !ui.config.receipt;
        let show_receipt = set(&|config| config.receipt = receipt);
        let titles: Vec<&'static str> = Id::ALL.iter().map(|id| id.title()).collect();
        let start = Id::from_key(&ui.config.start).unwrap_or(Id::Home).title();
        let config = ui.config.clone();
        let general = ui.group(
            "",
            vec![
                ui.row(
                    "Open at launch",
                    "",
                    ui.pick(titles, Some(start), move |title| {
                        let mut config = config.clone();
                        if let Some(id) = Id::ALL.iter().find(|id| id.title() == title) {
                            id.key().clone_into(&mut config.start);
                        }
                        Message::Configure(Box::new(config))
                    }),
                ),
                ui.toggle(
                    "Show commands",
                    "The bar with the last command and Undo",
                    ui.config.receipt,
                    Some(move |_| show_receipt.clone()),
                ),
            ],
        );

        let order = Section::ordered(ui.config);
        let count = order.len();
        let sections = order
            .iter()
            .enumerate()
            .map(|(index, section)| {
                let moved = |forward| {
                    set(&|config| {
                        let mut keys: Vec<String> =
                            order.iter().map(|s| s.key().to_owned()).collect();
                        shift(&mut keys, index, forward);
                        config.sections = keys;
                    })
                };
                ui.icon_row(
                    section.icon(),
                    Tone::Mute,
                    section.title(),
                    "",
                    arrows(index, count, &moved),
                )
            })
            .collect();

        let used: Vec<Tile> = ui
            .config
            .tiles
            .iter()
            .filter_map(|key| Tile::ALL.into_iter().find(|tile| tile.key() == key))
            .collect();
        let unused = Tile::ALL.into_iter().filter(|tile| !used.contains(tile));
        let mut tiles: Vec<_> = used
            .iter()
            .enumerate()
            .map(|(index, tile)| {
                let moved = |forward| set(&|config| shift(&mut config.tiles, index, forward));
                let remove =
                    set(&|config| config.tiles.retain(|key| key != tile.key()));
                ui.row(
                    tile.label(),
                    "",
                    ui.actions(vec![
                        arrows(index, used.len(), &moved),
                        ui.switch(true, Some(move |_| remove.clone())),
                    ]),
                )
            })
            .collect();
        tiles.extend(unused.map(|tile| {
            let add = set(&|config| config.tiles.push(tile.key().to_owned()));
            ui.row(tile.label(), "", ui.switch(false, Some(move |_: bool| add.clone())))
        }));

        let panels = Id::ALL
            .iter()
            // Hiding Home or this panel would leave no way back.
            .filter(|id| !matches!(id, Id::Home | Id::Theme))
            .map(|id| {
                let hidden = ui.config.hidden.iter().any(|key| key == id.key());
                let toggled = set(&|config| {
                    if hidden {
                        config.hidden.retain(|key| key != id.key());
                    } else {
                        config.hidden.push(id.key().to_owned());
                    }
                });
                ui.toggle(
                    id.title(),
                    id.section().title(),
                    !hidden,
                    Some(move |_| toggled.clone()),
                )
            })
            .collect();

        vec![
            general,
            ui.columns(
                ui.stack(vec![ui.group("Sections", sections), ui.group("Home tiles", tiles)]),
                ui.group("Panels", panels),
            ),
        ]
    }
}

/// The theme an edit starts from: the theme itself when it belongs to the user,
/// or a copy under a new name when it is built in.
fn copy(ui: Ui<'_>) -> Spec {
    let current = ui.theme.spec();
    if current.path.is_some() {
        return current.clone();
    }
    let name = format!("My {}", current.name);
    Spec {
        id: ui.library.unique_id(&name),
        base: Some(current.id.clone()),
        author: String::new(),
        name,
        ..current.clone()
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        if self.fonts.0.is_empty() {
            return Task::perform(sys::desktop::fonts(), |(ui, mono)| Message::Fonts(ui, mono));
        }
        Task::none()
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Fonts(ui, mono) => {
                self.fonts = (ui, mono);
                Effect::none()
            }
            Message::Tab(tab) => {
                self.tab = tab;
                Effect::none()
            }
            Message::Use(config) => {
                self.confirm_delete = false;
                Effect::request(Request::Configure(config))
            }
            Message::Configure(config) => Effect::request(Request::Configure(config)),
            Message::Edit(spec) => {
                self.draft = Some(*spec);
                self.role = None;
                self.preview()
            }
            Message::Role(index) => {
                self.role = (self.role != Some(index)).then_some(index);
                if let (Some(index), Some(draft)) = (self.role, &self.draft) {
                    self.color = ColorEdit::new((ROLES[index].get)(&draft.colors));
                }
                Effect::none()
            }
            Message::Color(message) => match (self.color.update(message), self.role) {
                (Some(color), Some(index)) => {
                    self.change(|draft| (ROLES[index].set)(&mut draft.colors, color))
                }
                _ => Effect::none(),
            },
            Message::Knob(index, value) => {
                self.change(|draft| (KNOBS[index].set)(&mut draft.metrics, value))
            }
            Message::RailLabels(on) => self.change(|draft| draft.rail_labels = on),
            Message::Name(name) => self.change(|draft| draft.name = name),
            Message::Author(author) => self.change(|draft| draft.author = author),
            Message::UiFont(font) => self.change(|draft| draft.fonts.ui = font.0),
            Message::MonoFont(font) => self.change(|draft| draft.fonts.mono = font.0),
            Message::Save => {
                let Some(draft) = &self.draft else { return Effect::none() };
                let dir = library::dir();
                match Library::load(&dir).save(&dir, draft) {
                    Ok(_) => {
                        let mut config = Config::load();
                        config.follow_desktop = false;
                        config.theme.clone_from(&draft.id);
                        self.draft = None;
                        Effect::request(Request::ReloadThemes)
                            .and(Task::done(Message::Configure(Box::new(config))))
                    }
                    Err(error) => Effect::request(Request::Report(error)),
                }
            }
            Message::Discard => {
                self.draft = None;
                Effect::request(Request::ReloadThemes)
            }
            Message::AskDelete(ask) => {
                self.confirm_delete = ask;
                Effect::none()
            }
            Message::Delete(path) => {
                self.confirm_delete = false;
                match std::fs::remove_file(&path) {
                    Ok(()) => Effect::request(Request::ReloadThemes),
                    Err(error) => Effect::request(Request::Report(error.into())),
                }
            }
            Message::OpenFolder => {
                let dir = library::dir();
                if let Err(error) = std::fs::create_dir_all(&dir) {
                    return Effect::request(Request::Report(error.into()));
                }
                Effect::run(Cmd::new("xdg-open").arg(dir.display().to_string()).detached())
            }
            Message::ImportPath(path) => {
                self.import = path;
                Effect::none()
            }
            Message::Import => {
                let dir = library::dir();
                let source = match self.import.trim().strip_prefix("~/") {
                    Some(rest) => sys::home().join(rest),
                    None => PathBuf::from(self.import.trim()),
                };
                match Library::load(&dir).import(&dir, &source) {
                    Ok(id) => {
                        self.import.clear();
                        let mut config = Config::load();
                        config.follow_desktop = false;
                        config.theme = id;
                        Effect::request(Request::ReloadThemes)
                            .and(Task::done(Message::Configure(Box::new(config))))
                    }
                    Err(error) => Effect::request(Request::Report(error)),
                }
            }
            Message::Settled => Effect::none(),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        if let Some(draft) = &self.draft {
            let ready = !draft.name.trim().is_empty();
            let actions = ui.actions(vec![
                ui.button("Discard", Kind::Link, Some(Message::Discard)),
                ui.button("Save theme", Kind::Primary, ready.then_some(Message::Save)),
            ]);
            return ui.panel(
                "Edit theme",
                "Changes show right away and are kept when you save",
                Some(actions),
                self.editor(ui, draft),
            );
        }
        let tabs = ui.segments(
            [(Tab::Themes, "Themes"), (Tab::Layout, "Layout")],
            &self.tab,
            Message::Tab,
            true,
        );
        let body = match self.tab {
            Tab::Themes => self.gallery(ui),
            Tab::Layout => self.layout(ui),
        };
        ui.panel("Theme", "How this program looks and what it shows", Some(tabs), body)
    }

    fn summary(&self) -> String {
        "Colours, fonts and layout".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifting_stops_at_the_ends() {
        let mut list = [1, 2, 3];
        shift(&mut list, 0, false);
        shift(&mut list, 2, true);
        assert_eq!(list, [1, 2, 3]);
        shift(&mut list, 0, true);
        assert_eq!(list, [2, 1, 3]);
    }
}
