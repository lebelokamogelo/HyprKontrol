use iced::widget::{column, row};
use iced::{Center, Task};

use super::{Effect, Panel, Watch, quietly};
use crate::icon::Icon;
use crate::sys::audio::{self, Audio, Device, Direction, Stream};
use crate::sys::{Change, Cmd, Result};
use crate::theme::Tone;
use crate::ui::{Element, Throttle, Ui};

/// What a slider controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Device(Direction, String),
    Stream(u32),
}

impl Target {
    fn command(&self, percent: f32) -> Cmd {
        match self {
            Target::Device(direction, name) => audio::set_volume(*direction, name, percent),
            Target::Stream(index) => audio::set_stream_volume(*index, percent),
        }
    }
}

#[derive(Default)]
pub struct State {
    audio: Option<Result<Audio>>,
    /// The slider being dragged, its value, and the value before the drag.
    dragging: Option<(Target, f32, f32)>,
    throttle: Throttle<(Target, f32)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Result<Audio>>),
    Drag(Target, f32, f32),
    Release,
    Sent,
    Mute(Direction, String, bool),
    MuteStream(u32, bool),
    Default(Direction, String),
    Profile(String, String, String),
}

#[derive(Debug, Clone, PartialEq)]
struct Profile {
    id: String,
    label: String,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl State {
    fn audio(&self) -> Option<&Audio> {
        self.audio.as_ref()?.as_ref().ok()
    }

    /// The value to draw: the drag position while dragging, else the real one.
    fn shown(&self, target: &Target, actual: f32) -> f32 {
        match &self.dragging {
            Some((dragged, value, _)) if dragged == target => *value,
            _ => actual,
        }
    }

    fn volume<'a>(
        &self,
        ui: Ui<'a>,
        icon: Icon,
        target: Target,
        actual: f32,
        muted: bool,
        mute: Message,
    ) -> Element<'a, Message> {
        let value = self.shown(&target, actual);
        let tone = if muted { Tone::Mute } else { Tone::Text };
        let icon = if muted { Icon::Muted } else { icon };
        let slider = ui.slider(
            0.0..=100.0,
            1.0,
            value.min(100.0),
            move |percent| Message::Drag(target.clone(), percent, actual),
            Message::Release,
        );
        let content = row![
            ui.icon_button(icon, tone, Some(mute)),
            slider,
            ui.dim(format!("{value:.0}%")).width(48).align_x(iced::Right),
        ]
        .spacing(14)
        .align_y(Center);
        ui.cell(content)
    }

    fn devices<'a>(
        &self,
        ui: Ui<'a>,
        title: &'static str,
        direction: Direction,
        devices: &'a [Device],
    ) -> Element<'a, Message> {
        let mut rows = Vec::new();
        if devices.is_empty() {
            rows.push(ui.cell(ui.dim("Nothing connected")));
        }
        if devices.len() > 1 {
            rows.extend(devices.iter().map(|device| {
                let icon = match (direction, device.headphones) {
                    (Direction::Input, _) => Icon::Mic,
                    (Direction::Output, true) => Icon::Headphones,
                    (Direction::Output, false) => Icon::Sound,
                };
                let pick = Message::Default(direction, device.name.clone());
                ui.option(icon, device.label.as_str(), "", device.default, pick)
            }));
        }
        if let Some(device) = devices.iter().find(|device| device.default) {
            let icon = if direction == Direction::Input { Icon::Mic } else { Icon::Sound };
            rows.push(self.volume(
                ui,
                icon,
                Target::Device(direction, device.name.clone()),
                device.volume,
                device.muted,
                Message::Mute(direction, device.name.clone(), !device.muted),
            ));
        }
        ui.group(title, rows)
    }

    fn streams<'a>(&self, ui: Ui<'a>, streams: &'a [Stream]) -> Element<'a, Message> {
        let mut rows: Vec<_> = streams
            .iter()
            .map(|stream| {
                let slider = self.volume(
                    ui,
                    Icon::Sound,
                    Target::Stream(stream.index),
                    stream.volume,
                    stream.muted,
                    Message::MuteStream(stream.index, !stream.muted),
                );
                column![ui.cell(ui.strong(stream.app.as_str())), slider].into()
            })
            .collect();
        if rows.is_empty() {
            rows.push(ui.cell(ui.dim("Nothing is playing")));
        }
        ui.group("Applications", rows)
    }

    fn cards<'a>(&self, ui: Ui<'a>, audio: &'a Audio) -> Element<'a, Message> {
        let rows = audio
            .cards
            .iter()
            .map(|card| {
                let profiles: Vec<_> = card
                    .profiles
                    .iter()
                    .map(|(id, label)| Profile { id: id.clone(), label: label.clone() })
                    .collect();
                let selected = profiles.iter().find(|p| p.id == card.profile).cloned();
                let name = card.name.clone();
                let before = card.profile.clone();
                let choose = move |profile: Profile| {
                    Message::Profile(name.clone(), before.clone(), profile.id)
                };
                let pick = ui.dropdown(profiles, selected, choose, iced::Fill);
                ui.cell(column![ui.strong(card.label.as_str()), pick].spacing(8))
            })
            .collect();
        ui.group("Hardware profiles", rows)
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(audio::load(), |audio| Message::Loaded(Box::new(audio)))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(audio) => {
                self.audio = Some(*audio);
                Effect::none()
            }
            Message::Drag(target, value, actual) => {
                let before = match &self.dragging {
                    Some((dragged, _, before)) if *dragged == target => *before,
                    _ => actual,
                };
                self.dragging = Some((target.clone(), value, before));
                match self.throttle.push((target, value)) {
                    Some((target, value)) => {
                        Effect::task(quietly(target.command(value), Message::Sent))
                    }
                    None => Effect::none(),
                }
            }
            Message::Sent => match self.throttle.done() {
                Some((target, value)) => {
                    Effect::task(quietly(target.command(value), Message::Sent))
                }
                None => Effect::none(),
            },
            Message::Release => {
                self.throttle.clear();
                match self.dragging.take() {
                    Some((target, value, before)) => Effect::change(
                        Change::new(target.command(value)).undo(target.command(before)),
                    ),
                    None => Effect::none(),
                }
            }
            Message::Mute(direction, name, muted) => Effect::change(
                Change::new(audio::set_mute(direction, &name, muted))
                    .undo(audio::set_mute(direction, &name, !muted)),
            ),
            Message::MuteStream(index, muted) => Effect::change(
                Change::new(audio::set_stream_mute(index, muted))
                    .undo(audio::set_stream_mute(index, !muted)),
            ),
            Message::Default(direction, name) => {
                let before = self.audio().and_then(|audio| match direction {
                    Direction::Output => audio.output(),
                    Direction::Input => audio.input(),
                });
                let change = Change::new(audio::set_default(direction, &name));
                Effect::change(match before {
                    Some(device) => change.undo(audio::set_default(direction, &device.name)),
                    None => change,
                })
            }
            Message::Profile(card, before, after) => Effect::change(
                Change::new(audio::set_profile(&card, &after))
                    .undo(audio::set_profile(&card, &before)),
            ),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let body = match &self.audio {
            None => vec![ui.loading()],
            Some(Err(error)) => vec![ui.failure(error.to_string())],
            Some(Ok(audio)) => vec![
                ui.columns(
                    self.devices(ui, "Output", Direction::Output, &audio.outputs),
                    self.devices(ui, "Input", Direction::Input, &audio.inputs),
                ),
                ui.columns(self.streams(ui, &audio.streams), self.cards(ui, audio)),
            ],
        };
        ui.panel("Sound", self.summary(), None, body)
    }

    fn summary(&self) -> String {
        match self.audio().and_then(Audio::output) {
            Some(device) if device.muted => format!("{}, muted", device.label),
            Some(device) => format!("{}, {:.0}%", device.label, device.volume),
            None => String::new(),
        }
    }

    fn watch(&self) -> Watch {
        Watch::Command(&["pactl", "subscribe"])
    }
}
