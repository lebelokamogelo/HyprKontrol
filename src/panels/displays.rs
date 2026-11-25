use std::time::{Duration, Instant};

use iced::widget::{column, row};
use iced::{Center, Fill, Rectangle, Subscription, Task};

use super::{Effect, Panel, SETTLE, Watch, follow, quietly};
use crate::sys::hypr::{self, Edit, Rule};
use crate::sys::levels::Source;
use crate::sys::monitor::{self, Mode, Monitor};
use crate::sys::{Op, Result, power};
use crate::ui::{Element, Kind, Throttle, Ui, chart};

/// How long a risky change stays before it is reverted. A mode the display
/// cannot show leaves the user without any way to click Undo.
const CONFIRM_SECONDS: u8 = 15;

#[derive(Default)]
pub struct State {
    monitors: Option<Result<Vec<Monitor>>>,
    selected: usize,
    brightness: Option<f32>,
    brightness_before: Option<f32>,
    throttle: Throttle<f32>,
    /// Reads because of a change, one at a time.
    reads: Throttle<()>,
    /// When the slider was last let go.
    released: Option<Instant>,
    /// Rule to go back to, the rule on trial, and the seconds left to keep it.
    trial: Option<(Rule, Rule, u8)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<Vec<Monitor>>, Option<f32>),
    Select(usize),
    Mode(Mode),
    Scale(Scale),
    Transform(&'static str),
    Mirror(String),
    Enable(bool),
    Move(Side),
    Tried(Result<()>),
    Keep,
    Revert,
    Countdown,
    Reverted,
    Brightness(f32),
    BrightnessDone,
    Sent,
    Changed,
    Seen(Option<f32>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale(f64);

impl std::fmt::Display for Scale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.0}%", self.0 * 100.0)
    }
}

/// Where a display sits relative to the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Above,
    Below,
}

impl State {
    fn list(&self) -> &[Monitor] {
        match &self.monitors {
            Some(Ok(monitors)) => monitors,
            _ => &[],
        }
    }

    fn current(&self) -> Option<&Monitor> {
        self.list().get(self.selected)
    }

    /// Applies a rule live, without saving it, and starts the countdown.
    fn attempt(&mut self, edit: impl FnOnce(&mut Rule)) -> Effect<Message> {
        let Some(monitor) = self.current() else { return Effect::none() };
        let before = monitor.rule();
        let mut after = before.clone();
        edit(&mut after);
        if after == before {
            return Effect::none();
        }
        let lua = after.lua();
        // A second change during a trial still goes back to the last kept rule.
        let origin = self.trial.take().map_or(before, |(origin, ..)| origin);
        self.trial = Some((origin, after, CONFIRM_SECONDS));
        Effect::task(Task::perform(async move { hypr::eval(&lua).await }, Message::Tried))
    }

    fn revert(&mut self) -> Effect<Message> {
        let Some((before, ..)) = self.trial.take() else { return Effect::none() };
        let lua = before.lua();
        Effect::task(Task::perform(async move { hypr::eval(&lua).await }, |_| Message::Reverted))
    }

    /// Places the selected display next to the first other one.
    fn place(&mut self, side: Side) -> Effect<Message> {
        let Some(monitor) = self.current() else { return Effect::none() };
        let Some(anchor) =
            self.list().iter().find(|m| m.name != monitor.name && !m.disabled)
        else {
            return Effect::none();
        };
        let (width, height) = monitor.logical();
        let (anchor_width, anchor_height) = anchor.logical();
        let (x, y) = (f64::from(anchor.x), f64::from(anchor.y));
        let (x, y) = match side {
            Side::Left => (x - width, y),
            Side::Right => (x + anchor_width, y),
            Side::Above => (x, y - height),
            Side::Below => (x, y + anchor_height),
        };
        self.attempt(|rule| rule.position = format!("{x:.0}x{y:.0}"))
    }

    fn arrangement<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let screens = self
            .list()
            .iter()
            .map(|monitor| {
                let (width, height) = monitor.logical();
                let area = Rectangle {
                    x: monitor.x as f32,
                    y: monitor.y as f32,
                    width: width as f32,
                    height: height as f32,
                };
                (area, monitor.title())
            })
            .collect();
        let screens =
            chart::Screens { screens, selected: self.selected, on_select: Message::Select };
        let mut rows = vec![screens.view(220.0)];
        if self.list().len() > 1 {
            let place = |label, side| {
                ui.button(label, Kind::Secondary, Some(Message::Move(side)))
            };
            rows.push(ui.row(
                "Position",
                "Relative to the other display",
                ui.actions(vec![
                    place("Left", Side::Left),
                    place("Right", Side::Right),
                    place("Above", Side::Above),
                    place("Below", Side::Below),
                ]),
            ));
        }
        ui.group("", rows)
    }

    fn settings<'a>(&'a self, ui: Ui<'a>, monitor: &'a Monitor) -> Element<'a, Message> {
        let mut rows = Vec::new();
        let others: Vec<_> = self.list().iter().filter(|m| m.name != monitor.name).collect();
        if !others.is_empty() {
            // The last display cannot be turned off, there would be no way back.
            let can_disable = monitor.disabled || others.iter().any(|m| !m.disabled);
            rows.push(ui.toggle(
                "Use this display",
                "",
                !monitor.disabled,
                can_disable.then_some(Message::Enable),
            ));
            let mut targets = vec!["Nothing".to_owned()];
            targets.extend(others.iter().map(|m| m.name.clone()));
            let mirror = monitor.rule().mirror;
            let selected = if mirror.is_empty() { targets[0].clone() } else { mirror };
            rows.push(ui.row("Mirror", "", ui.pick(targets, Some(selected), Message::Mirror)));
        }
        rows.push(ui.row(
            "Resolution",
            "",
            ui.pick(monitor.modes(), monitor.mode(), Message::Mode),
        ));
        let mut scales: Vec<Scale> =
            monitor::valid_scales(monitor.width, monitor.height).into_iter().map(Scale).collect();
        let current = Scale(monitor.scale);
        if !scales.iter().any(|s| (s.0 - current.0).abs() < 0.001) {
            scales.push(current);
            scales.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let selected = scales.iter().copied().find(|s| (s.0 - current.0).abs() < 0.001);
        rows.push(ui.row(
            "Scale",
            "Only sizes that stay sharp are listed",
            ui.pick(scales, selected, Message::Scale),
        ));
        let rotations: Vec<_> = monitor::TRANSFORMS.to_vec();
        let rotation = rotations.get(usize::from(monitor.transform)).copied();
        rows.push(ui.row("Rotation", "", ui.pick(rotations, rotation, Message::Transform)));
        if let (Some(percent), true) = (self.brightness, monitor.is_internal()) {
            rows.push(ui.slide(
                "Brightness",
                1.0..=100.0,
                1.0,
                percent,
                format!("{percent:.0}%"),
                Message::Brightness,
                Message::BrightnessDone,
            ));
        }
        let title = format!("{} ({})", monitor.title(), monitor.name);
        ui.group(title, rows)
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(
            async { tokio::join!(monitor::load(), power::brightness()) },
            |(monitors, brightness)| Message::Loaded(monitors, brightness),
        )
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(monitors, brightness) => {
                self.monitors = Some(monitors);
                if self.brightness_before.is_none() {
                    self.brightness = brightness;
                }
                self.selected = self.selected.min(self.list().len().saturating_sub(1));
                Effect::none()
            }
            Message::Select(index) => {
                self.selected = index;
                Effect::none()
            }
            Message::Mode(mode) => self.attempt(|rule| {
                rule.mode = format!("{}x{}@{:.2}", mode.width, mode.height, mode.hz);
            }),
            Message::Scale(scale) => self.attempt(|rule| rule.scale = scale.0),
            Message::Transform(name) => self.attempt(|rule| {
                let index = monitor::TRANSFORMS.iter().position(|known| *known == name);
                rule.transform = index.unwrap_or(0) as u8;
            }),
            Message::Mirror(target) => self.attempt(|rule| {
                rule.mirror = if target == "Nothing" { String::new() } else { target };
            }),
            Message::Enable(on) => self.attempt(|rule| rule.disabled = !on),
            Message::Move(side) => self.place(side),
            Message::Tried(Ok(())) => Effect::task(Task::batch([self.load(), tick()])),
            Message::Tried(Err(error)) => {
                self.trial = None;
                Effect::request(super::Request::Report(error)).and(self.load())
            }
            Message::Countdown => match &mut self.trial {
                Some((_, _, 0 | 1)) => self.revert(),
                Some((_, _, seconds)) => {
                    *seconds -= 1;
                    Effect::task(tick())
                }
                None => Effect::none(),
            },
            Message::Keep => match self.trial.take() {
                Some((before, after, _)) => Effect::change(
                    crate::sys::Change::new(Op::Hypr(vec![Edit::Monitor(after)]))
                        .undo(Op::Hypr(vec![Edit::Monitor(before)])),
                ),
                None => Effect::none(),
            },
            Message::Revert => self.revert(),
            Message::Reverted => Effect::task(self.load()),
            Message::Brightness(percent) => {
                if self.brightness_before.is_none() {
                    self.brightness_before = self.brightness;
                }
                self.brightness = Some(percent);
                match self.throttle.push(percent) {
                    Some(value) => {
                        Effect::task(quietly(power::set_brightness(value), Message::Sent))
                    }
                    None => Effect::none(),
                }
            }
            Message::Sent => match self.throttle.done() {
                Some(value) => Effect::task(quietly(power::set_brightness(value), Message::Sent)),
                None => Effect::none(),
            },
            Message::Changed => match self.reads.push(()) {
                Some(()) => Effect::task(read()),
                None => Effect::none(),
            },
            Message::Seen(seen) => {
                let settled = self.released.is_none_or(|at| at.elapsed() > SETTLE);
                if seen.is_some() && settled && self.brightness_before.is_none() {
                    self.brightness = seen;
                }
                match self.reads.done() {
                    Some(()) => Effect::task(read()),
                    None => Effect::none(),
                }
            }
            Message::BrightnessDone => {
                self.throttle.clear();
                self.released = Some(Instant::now());
                match (self.brightness_before.take(), self.brightness) {
                    (Some(before), Some(after)) => Effect::change(
                        crate::sys::Change::new(power::set_brightness(after))
                            .undo(power::set_brightness(before)),
                    ),
                    _ => Effect::none(),
                }
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let mut body = Vec::new();
        if let Some((_, _, seconds)) = &self.trial {
            let question = column![
                ui.bold("Keep this setting?"),
                ui.small(format!("Going back in {seconds} seconds")),
            ]
            .width(Fill);
            let controls = ui.actions(vec![
                ui.button("Go back", Kind::Secondary, Some(Message::Revert)),
                ui.button("Keep", Kind::Primary, Some(Message::Keep)),
            ]);
            body.push(ui.card(row![question, controls].spacing(16).align_y(Center)));
        }
        match &self.monitors {
            None => body.push(ui.loading()),
            Some(Err(error)) => body.push(ui.failure(error.to_string())),
            Some(Ok(_)) => {
                body.push(self.arrangement(ui));
                if let Some(monitor) = self.current() {
                    body.push(self.settings(ui, monitor));
                }
            }
        }
        ui.panel("Displays", self.summary(), None, body)
    }

    fn summary(&self) -> String {
        match self.list() {
            [] => String::new(),
            [only] => format!("{} × {} at {:.0}%", only.width, only.height, only.scale * 100.0),
            several => format!("{} displays", several.len()),
        }
    }

    fn watch(&self) -> Watch {
        Watch::Every(Duration::from_secs(4))
    }

    fn events(&self) -> Subscription<Message> {
        follow(Source::Brightness).map(|()| Message::Changed)
    }
}

fn read() -> Task<Message> {
    Task::perform(power::brightness(), Message::Seen)
}

fn tick() -> Task<Message> {
    // The timer is created inside the future. Creating it here would require a
    // runtime while the panel updates.
    Task::future(async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Message::Countdown
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        let monitor = serde_json::from_value(serde_json::json!({
            "name": "eDP-1", "make": "AU", "model": "0x3DA3", "width": 1920, "height": 1080,
            "refreshRate": 60.038, "x": 0, "y": 0, "scale": 1.25, "transform": 0,
            "disabled": false, "mirrorOf": "none", "availableModes": ["1920x1080@60.04Hz"]
        }));
        State { monitors: Some(Ok(vec![monitor.unwrap()])), ..State::default() }
    }

    #[test]
    fn an_unconfirmed_change_reverts_by_itself() {
        let mut state = state();
        let _ = state.update(Message::Scale(Scale(1.0)));
        assert!(state.trial.is_some());
        for _ in 0..CONFIRM_SECONDS {
            assert!(state.trial.is_some(), "reverted too early");
            let effect = state.update(Message::Countdown);
            assert!(effect.change.is_none(), "nothing may be saved during a trial");
        }
        assert!(state.trial.is_none());
    }

    #[test]
    fn only_a_kept_change_is_saved_and_it_undoes_to_the_original() {
        let mut state = state();
        let _ = state.update(Message::Scale(Scale(1.0)));
        // A second attempt before the first is kept.
        let _ = state.update(Message::Scale(Scale(1.5)));
        let change = state.update(Message::Keep).change.unwrap();
        let rule = |op: &Op| match op {
            Op::Hypr(edits) => match &edits[0] {
                Edit::Monitor(rule) => rule.scale,
                other => panic!("unexpected {other:?}"),
            },
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(rule(&change.apply), 1.5);
        assert_eq!(rule(&change.undo.unwrap()), 1.25);
    }

    #[test]
    fn choosing_the_current_value_is_not_a_change() {
        let mut state = state();
        let _ = state.update(Message::Scale(Scale(1.25)));
        assert!(state.trial.is_none());
    }
}
