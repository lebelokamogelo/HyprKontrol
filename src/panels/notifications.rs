use iced::Task;
use serde_json::{Value, json};

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::{Cmd, desktop, packages};
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    loaded: bool,
    dnd: Option<bool>,
    config: Option<Value>,
    dragging: Option<(&'static str, f32)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Option<bool>, Box<Option<Value>>),
    Dnd(bool),
    Text(&'static str, &'static str),
    Flag(&'static str, bool),
    Drag(&'static str, f32),
    Release,
    Clear,
    Test,
    Install,
}

const HORIZONTAL: [(&str, &str); 3] = [("left", "Left"), ("center", "Centre"), ("right", "Right")];
const VERTICAL: [(&str, &str); 3] = [("top", "Top"), ("center", "Centre"), ("bottom", "Bottom")];
const IMAGES: [(&str, &str); 3] =
    [("always", "Always"), ("when-available", "When there is one"), ("never", "Never")];

impl State {
    fn set(&self, key: &str, value: Value) -> Effect<Message> {
        match &self.config {
            Some(config) => Effect::attempt(desktop::set_notification(config, key, value)),
            None => Effect::none(),
        }
    }

    fn seconds<'a>(
        &self,
        ui: Ui<'a>,
        config: &Value,
        key: &'static str,
        label: &'static str,
    ) -> Element<'a, Message> {
        let value = match self.dragging {
            Some((dragged, value)) if dragged == key => value,
            _ => config[key].as_f64().unwrap_or(0.0) as f32,
        };
        let shown = if value < 1.0 { "Stays".to_owned() } else { format!("{value:.0} s") };
        ui.slide(
            label,
            0.0..=30.0,
            1.0,
            value,
            shown,
            move |value| Message::Drag(key, value),
            Message::Release,
        )
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(desktop::do_not_disturb(), |dnd| {
            Message::Loaded(dnd, Box::new(desktop::notifications()))
        })
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(dnd, config) => {
                self.loaded = true;
                self.dnd = dnd;
                self.config = *config;
                Effect::none()
            }
            Message::Dnd(on) => Effect::change(desktop::set_do_not_disturb(on)),
            Message::Text(key, value) => self.set(key, json!(value)),
            Message::Flag(key, value) => self.set(key, json!(value)),
            Message::Drag(key, value) => {
                self.dragging = Some((key, value));
                Effect::none()
            }
            Message::Release => match self.dragging.take() {
                Some((key, value)) => self.set(key, json!(value as i64)),
                None => Effect::none(),
            },
            Message::Clear => Effect::run(Cmd::new("swaync-client").arg("--close-all")),
            Message::Test => Effect::run(
                Cmd::new("notify-send").args(["HyprKontrol", "This is a test notification"]),
            ),
            Message::Install => Effect::checked(packages::install(&["swaync"])),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        const TITLE: &str = "Notifications";
        if !self.loaded {
            return ui.panel(TITLE, "", None, vec![ui.loading()]);
        }
        let actions = ui.actions(vec![
            ui.button("Send a test", Kind::Secondary, Some(Message::Test)),
            ui.button("Clear all", Kind::Secondary, self.dnd.map(|_| Message::Clear)),
        ]);
        let Some(dnd) = self.dnd else {
            let empty = ui.empty(
                Icon::Bell,
                "SwayNC is not running",
                "This panel manages the notification centre that CachyOS ships for Hyprland.",
                Some(ui.button("Install SwayNC", Kind::Primary, Some(Message::Install))),
            );
            return ui.panel(TITLE, "", None, vec![empty]);
        };
        let mut body = vec![ui.group(
            "",
            vec![ui.toggle(
                "Do not disturb",
                "Notifications are collected without popping up",
                dnd,
                Some(Message::Dnd),
            )],
        )];
        if let Some(config) = &self.config {
            let text = |key: &str| config[key].as_str().unwrap_or_default();
            let flag = |key: &'static str, label: &'static str, hint: &'static str| {
                ui.toggle(
                    label,
                    hint,
                    config[key].as_bool().unwrap_or(false),
                    Some(move |on| Message::Flag(key, on)),
                )
            };
            let place = ui.group(
                "Place",
                vec![
                    ui.row(
                        "Side",
                        "",
                        ui.choose(&HORIZONTAL, text("positionX"), |v| {
                            Message::Text("positionX", v)
                        }),
                    ),
                    ui.row(
                        "Edge",
                        "",
                        ui.choose(&VERTICAL, text("positionY"), |v| Message::Text("positionY", v)),
                    ),
                    ui.row(
                        "Images",
                        "",
                        ui.choose(&IMAGES, text("image-visibility"), |v| {
                            Message::Text("image-visibility", v)
                        }),
                    ),
                ],
            );
            let time = ui.group(
                "Hide after",
                vec![
                    self.seconds(ui, config, "timeout", "Normal"),
                    self.seconds(ui, config, "timeout-low", "Low priority"),
                    self.seconds(ui, config, "timeout-critical", "Critical"),
                ],
            );
            let behaviour = ui.group(
                "Behaviour",
                vec![
                    flag("hide-on-clear", "Close the centre after clearing", ""),
                    flag("hide-on-action", "Close the centre after an action", ""),
                    flag("keyboard-shortcuts", "Keyboard shortcuts in the centre", ""),
                    flag("notification-inline-replies", "Reply inside notifications", ""),
                ],
            );
            body.push(ui.columns(ui.stack(vec![place, time]), behaviour));
        }
        ui.panel(TITLE, "", Some(actions), body)
    }

    fn summary(&self) -> String {
        match (self.loaded, self.dnd) {
            (false, _) => String::new(),
            (true, None) => "Not running".to_owned(),
            (true, Some(true)) => "Do not disturb".to_owned(),
            (true, Some(false)) => "On".to_owned(),
        }
    }
}
