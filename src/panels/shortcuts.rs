use iced::widget::{column, container, row};
use iced::{Center, Task};

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::hypr::{self, Bind, Edit, Shortcut, Store};
use crate::sys::{Change, Op};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    shortcuts: Option<Vec<Shortcut>>,
    custom: Vec<Bind>,
    query: String,
    keys: String,
    command: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Vec<Shortcut>, Vec<Bind>),
    Query(String),
    Keys(String),
    Command(String),
    Add,
    Remove(Bind),
}

const MODIFIERS: [(&str, &str); 7] = [
    ("SUPER", "Super"),
    ("CTRL", "Ctrl"),
    ("CONTROL", "Ctrl"),
    ("ALT", "Alt"),
    ("SHIFT", "Shift"),
    ("mouse:272", "Left button"),
    ("mouse:273", "Right button"),
];

/// Spells a key for display.
fn pretty(key: &str) -> String {
    let known = MODIFIERS.iter().find(|(name, _)| *name == key);
    known.map_or_else(|| key.trim_start_matches("XF86").to_owned(), |(_, label)| (*label).to_owned())
}

/// Normalises what a user types, such as `super+shift+f9`, to Hyprland's form.
/// Returns `None` unless there is exactly one key after the modifiers.
fn normalise(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split(['+', ' ']).filter(|part| !part.is_empty()).collect();
    let (key, modifiers) = parts.split_last()?;
    let modifiers: Option<Vec<&str>> = modifiers
        .iter()
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "super" | "win" | "meta" => Some("SUPER"),
            "ctrl" | "control" => Some("CTRL"),
            "alt" => Some("ALT"),
            "shift" => Some("SHIFT"),
            _ => None,
        })
        .collect();
    let is_modifier =
        MODIFIERS.iter().any(|(name, _)| name.eq_ignore_ascii_case(key)) || *key == "win";
    let valid = !is_modifier && key.chars().all(|c| c.is_ascii_alphanumeric() || "_:".contains(c));
    let mut keys = modifiers?;
    let key = if key.len() == 1 { key.to_ascii_uppercase() } else { (*key).to_owned() };
    keys.push(&key);
    valid.then(|| keys.join(" + "))
}

impl State {
    /// An existing shortcut on the same keys, in any order of modifiers.
    fn conflict(&self, keys: &str) -> Option<&str> {
        let sorted = |keys: &str| {
            let mut parts: Vec<String> = keys
                .split('+')
                .map(|p| p.trim().to_ascii_uppercase().replace("CONTROL", "CTRL"))
                .collect();
            parts.sort();
            parts
        };
        let wanted = sorted(keys);
        self.shortcuts
            .iter()
            .flatten()
            .find(|shortcut| sorted(&shortcut.keys) == wanted)
            .map(|shortcut| shortcut.action.as_str())
    }
}

fn keycaps<'a>(ui: Ui<'a>, keys: &str) -> Element<'a, Message> {
    let caps = keys.split('+').map(|key| ui.badge(pretty(key.trim())));
    row(caps).spacing(4).into()
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::future(async { Message::Loaded(hypr::shortcuts(), Store::load().binds) })
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(shortcuts, custom) => {
                self.shortcuts = Some(shortcuts);
                self.custom = custom;
                Effect::none()
            }
            Message::Query(query) => {
                self.query = query;
                Effect::none()
            }
            Message::Keys(keys) => {
                self.keys = keys;
                Effect::none()
            }
            Message::Command(command) => {
                self.command = command;
                Effect::none()
            }
            Message::Add => {
                let Some(keys) = normalise(&self.keys) else { return Effect::none() };
                let bind = Bind { keys, command: self.command.trim().to_owned() };
                self.keys.clear();
                self.command.clear();
                Effect::change(
                    Change::new(Op::Hypr(vec![Edit::Bind(bind.clone())]))
                        .undo(Op::Hypr(vec![Edit::Unbind(bind)])),
                )
            }
            Message::Remove(bind) => Effect::change(
                Change::new(Op::Hypr(vec![Edit::Unbind(bind.clone())]))
                    .undo(Op::Hypr(vec![Edit::Bind(bind)])),
            ),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(shortcuts) = &self.shortcuts else {
            return ui.panel("Shortcuts", "", None, vec![ui.loading()]);
        };
        let search =
            ui.search("Search shortcuts", "", |field| field, &self.query, Message::Query);

        let keys = normalise(&self.keys);
        let ready = keys.is_some() && !self.command.trim().is_empty();
        let keys_field = ui.field("Super + F9", &self.keys, Message::Keys);
        let command_field = ui
            .field("Command to run", &self.command, Message::Command)
            .on_submit_maybe(ready.then_some(Message::Add));
        let add = ui.button("Add", Kind::Primary, ready.then_some(Message::Add));
        let form: Element<'a, Message> = if ui.content < 520.0 {
            column![keys_field, command_field, add].spacing(8).into()
        } else {
            row![keys_field.width(150), command_field, add].spacing(8).align_y(Center).into()
        };
        let mut mine: Vec<_> = self
            .custom
            .iter()
            .map(|bind| {
                let remove = Message::Remove(bind.clone());
                let controls = row![
                    keycaps(ui, &bind.keys),
                    ui.icon_button(Icon::Close, Tone::Mute, Some(remove)),
                ]
                .spacing(10)
                .align_y(Center);
                ui.row(bind.command.as_str(), "", controls)
            })
            .collect();
        mine.push(ui.cell(form));
        if let Some(action) = keys.as_deref().and_then(|keys| self.conflict(keys)) {
            mine.push(ui.cell(
                ui.small(format!("Already used for: {action}"))
                    .style(crate::theme::style::ink(Tone::Warning)),
            ));
        }

        let query = self.query.to_lowercase();
        let rows: Vec<_> = shortcuts
            .iter()
            .filter(|shortcut| {
                query.is_empty()
                    || shortcut.action.to_lowercase().contains(&query)
                    || shortcut.keys.to_lowercase().contains(&query)
            })
            .map(|shortcut| {
                let keys = container(keycaps(ui, &shortcut.keys));
                ui.row(shortcut.action.as_str(), "", keys)
            })
            .collect();
        let listed = if rows.is_empty() {
            ui.empty(
                Icon::Search,
                "No shortcuts found",
                "Shortcuts are read from ~/.config/hypr/config/binds.lua.",
                None,
            )
        } else {
            ui.group("From your configuration", rows)
        };
        ui.panel(
            "Shortcuts",
            self.summary(),
            Some(search),
            vec![ui.group("Yours", mine), listed],
        )
    }

    fn summary(&self) -> String {
        match &self.shortcuts {
            Some(shortcuts) => format!("{} in use", shortcuts.len() + self.custom.len()),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_typed_keys() {
        assert_eq!(normalise("super+shift + f9").as_deref(), Some("SUPER + SHIFT + f9"));
        assert_eq!(normalise("win k").as_deref(), Some("SUPER + K"));
        assert_eq!(normalise("super"), None);
        assert_eq!(normalise("hyper + k"), None);
        assert_eq!(normalise("super + \"x"), None);
    }

    #[test]
    fn finds_conflicts_in_any_order() {
        let state = State {
            shortcuts: Some(vec![Shortcut {
                keys: "SUPER + CONTROL + Right".into(),
                action: "move".into(),
            }]),
            ..State::default()
        };
        assert_eq!(state.conflict("CTRL + SUPER + right"), Some("move"));
        assert_eq!(state.conflict("SUPER + Right"), None);
    }
}
