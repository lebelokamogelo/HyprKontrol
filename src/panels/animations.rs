use iced::Task;
use iced::widget::column;

use super::options::{OptionMessage, Options};
use super::{Effect, Panel};
use crate::sys::hypr::{self, Animated, Animation, Edit, Store};
use crate::sys::{Change, Op, Result};
use crate::ui::{Element, Ui};

const KEYS: &[&str] = &["animations.enabled"];

/// The leaves worth a control. The rest inherit from these.
const LEAVES: [(&str, &str); 8] = [
    ("global", "Everything"),
    ("windowsIn", "Opening windows"),
    ("windowsOut", "Closing windows"),
    ("windowsMove", "Moving windows"),
    ("workspaces", "Switching workspaces"),
    ("specialWorkspace", "Special workspace"),
    ("layers", "Bars and menus"),
    ("fade", "Fades"),
];

const STYLES: [(&str, &str); 5] = [
    ("", "Default"),
    ("slide", "Slide"),
    ("popin 80%", "Pop in"),
    ("fade", "Fade"),
    ("slidevert", "Slide vertically"),
];

#[derive(Default)]
pub struct State {
    options: Options,
    animations: Option<Result<(Vec<Animated>, Vec<String>)>>,
    dragging: Option<(&'static str, f32)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Option(OptionMessage),
    Loaded(Result<(Vec<Animated>, Vec<String>)>),
    Customise(&'static str, bool),
    Speed(&'static str, f32),
    SaveSpeed,
    Curve(&'static str, String),
    Style(&'static str, &'static str),
    Enabled(&'static str, bool),
}

impl State {
    fn find(&self, leaf: &str) -> Option<&Animated> {
        match &self.animations {
            Some(Ok((animations, _))) => animations.iter().find(|a| a.leaf == leaf),
            _ => None,
        }
    }

    /// The animation in effect. Leaves without their own follow "global".
    fn effective(&self, leaf: &str) -> Option<Animation> {
        let own = self.find(leaf)?;
        if own.overridden {
            return Some(own.spec.clone());
        }
        self.find("global").map(|global| global.spec.clone())
    }

    fn set(&self, leaf: &str, edit: impl FnOnce(&mut Animation)) -> Effect<Message> {
        let Some(before) = self.effective(leaf) else { return Effect::none() };
        let mut after = before.clone();
        edit(&mut after);
        let leaf = leaf.to_owned();
        let undo = if Store::load().animations.contains_key(&leaf) {
            Edit::Animate(leaf.clone(), before)
        } else {
            Edit::ResetAnimation(leaf.clone(), before)
        };
        Effect::change(
            Change::new(Op::Hypr(vec![Edit::Animate(leaf, after)])).undo(Op::Hypr(vec![undo])),
        )
    }

    fn leaf<'a>(
        &'a self,
        ui: Ui<'a>,
        (leaf, label): (&'static str, &'static str),
        curves: &'a [String],
    ) -> Option<Element<'a, Message>> {
        let own = self.find(leaf)?;
        let is_global = leaf == "global";
        let custom = own.overridden || is_global;
        let spec = self.effective(leaf)?;
        let header = if is_global {
            ui.row(label, "", ui.switch(spec.enabled, Some(move |on| Message::Enabled(leaf, on))))
        } else {
            ui.toggle(
                label,
                if custom { "" } else { "Same as everything" },
                custom && spec.enabled,
                Some(move |on| Message::Customise(leaf, on)),
            )
        };
        if !custom || !spec.enabled {
            return Some(header);
        }
        let speed = match self.dragging {
            Some((dragged, speed)) if dragged == leaf => speed,
            _ => spec.speed,
        };
        let mut body = column![
            header,
            ui.slide(
                "Duration",
                0.5..=15.0,
                0.5,
                speed,
                // Hyprland counts in tenths of a second.
                format!("{:.0} ms", speed * 100.0),
                move |value| Message::Speed(leaf, value),
                Message::SaveSpeed,
            ),
            ui.row(
                "Curve",
                "",
                ui.pick(curves.to_vec(), Some(spec.bezier.clone()), move |curve| {
                    Message::Curve(leaf, curve)
                }),
            ),
        ];
        if !is_global {
            body = body.push(ui.row(
                "Style",
                "",
                ui.choose(&STYLES, &spec.style, move |style| Message::Style(leaf, style)),
            ));
        }
        Some(body.into())
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::batch([
            Options::load(KEYS).map(Message::Option),
            Task::perform(hypr::animations(), Message::Loaded),
        ])
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Option(message) => self.options.update(message).map(Message::Option),
            Message::Loaded(animations) => {
                self.animations = Some(animations);
                Effect::none()
            }
            Message::Customise(leaf, on) | Message::Enabled(leaf, on) => {
                self.set(leaf, |spec| spec.enabled = on)
            }
            Message::Speed(leaf, speed) => {
                self.dragging = Some((leaf, speed));
                Effect::none()
            }
            Message::SaveSpeed => match self.dragging.take() {
                Some((leaf, speed)) => self.set(leaf, |spec| spec.speed = speed),
                None => Effect::none(),
            },
            Message::Curve(leaf, curve) => self.set(leaf, |spec| spec.bezier = curve),
            Message::Style(leaf, style) => {
                self.set(leaf, |spec| style.clone_into(&mut spec.style))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let (Some(animations), true) = (&self.animations, self.options.ready()) else {
            return ui.panel("Animations", "", None, vec![ui.loading()]);
        };
        let mut body: Vec<_> = self
            .options
            .toggle(ui, "animations.enabled", "Animations", "Turn off to save power", Message::Option)
            .map(|toggle| ui.group("", vec![toggle]))
            .into_iter()
            .collect();
        match animations {
            Err(error) => body.push(ui.failure(error.to_string())),
            Ok((_, curves)) if self.options.bool("animations.enabled") => {
                let mut cards: Vec<_> = LEAVES
                    .into_iter()
                    .filter_map(|entry| self.leaf(ui, entry, curves))
                    .map(|row| ui.group("", vec![row]))
                    .collect();
                let right = cards.split_off(cards.len().div_ceil(2));
                body.push(ui.columns(ui.stack(cards), ui.stack(right)));
            }
            Ok(_) => {}
        }
        ui.panel("Animations", "", None, body)
    }

    fn summary(&self) -> String {
        match (self.options.ready(), self.options.bool("animations.enabled")) {
            (false, _) => String::new(),
            (true, true) => "On".to_owned(),
            (true, false) => "Off".to_owned(),
        }
    }
}
