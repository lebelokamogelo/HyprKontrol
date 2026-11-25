use std::path::PathBuf;
use std::time::Duration;

use iced::Task;
use iced::widget::{column, container, row};

use super::{Effect, Panel, Watch, home};
use crate::icon::Icon;
use crate::sys::idle::{self, Stage};
use crate::sys::power::{self, Battery, Logind, Profiles};
use crate::sys::Change;
use crate::ui::{Element, Ui};

#[derive(Debug, Clone, Default)]
pub struct Data {
    battery: Option<Battery>,
    profiles: Profiles,
    idle: Option<String>,
    logind: Logind,
    has_lid: bool,
}

#[derive(Default)]
pub struct State {
    data: Option<Data>,
    limit: Option<f32>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Data>),
    Profile(String),
    BatteryAware(bool),
    Limit(f32),
    SaveLimit(PathBuf, u8),
    Idle(Stage, Option<u32>),
    Lid(&'static str),
    PowerKey(&'static str),
}

async fn load() -> Data {
    Data {
        battery: power::battery(),
        profiles: power::profiles().await,
        idle: idle::load(),
        logind: power::logind(),
        has_lid: power::has_lid(),
    }
}

/// A delay in a dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delay(pub Option<u32>);

impl std::fmt::Display for Delay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            None => f.write_str("Never"),
            Some(seconds) if seconds % 3600 == 0 => write!(f, "After {} h", seconds / 3600),
            Some(seconds) if seconds % 60 == 0 => write!(f, "After {} min", seconds / 60),
            Some(seconds) => write!(f, "After {seconds} s"),
        }
    }
}

/// Preset delays, with the current one added when it is not among them.
pub fn delays(current: Option<u32>) -> Vec<Delay> {
    let mut seconds: Vec<u32> =
        [1, 2, 3, 4, 5, 6, 7, 10, 15, 20, 30, 45, 60, 120].map(|minutes| minutes * 60).to_vec();
    seconds.extend(current);
    seconds.sort_unstable();
    seconds.dedup();
    seconds.into_iter().map(|s| Delay(Some(s))).chain([Delay(None)]).collect()
}

pub fn idle_rows<'a, M: Clone + 'a>(
    ui: Ui<'a>,
    text: &str,
    on_select: fn(Stage, Option<u32>) -> M,
) -> Vec<Element<'a, M>> {
    Stage::ALL
        .into_iter()
        .map(|stage| {
            let current = idle::timeout(text, stage);
            let pick = ui.pick(delays(current), Some(Delay(current)), move |delay| {
                on_select(stage, delay.0)
            });
            ui.row(stage.label(), "", pick)
        })
        .collect()
}

fn describe(profile: &str) -> (Icon, &'static str, &'static str) {
    match profile {
        "power-saver" => (Icon::Leaf, "Power saver", "Caps clock speed for longer battery life"),
        "performance" => {
            (Icon::Bolt, "Performance", "Holds high clocks, with more heat and fan")
        }
        _ => (Icon::Gauge, "Balanced", "Full speed when something needs it"),
    }
}

impl State {
    fn profiles<'a>(&self, ui: Ui<'a>, data: &Data) -> Element<'a, Message> {
        let mut rows: Vec<_> = data
            .profiles
            .available
            .iter()
            .map(|name| {
                let (icon, label, hint) = describe(name);
                let selected = *name == data.profiles.active;
                ui.option(icon, label, hint, selected, Message::Profile(name.clone()))
            })
            .collect();
        if rows.is_empty() {
            rows.push(ui.cell(ui.dim("Install power-profiles-daemon to switch modes")));
        }
        if let Some(on) = data.profiles.battery_aware {
            rows.push(ui.toggle(
                "Adapt to the battery",
                "Saves more as the charge drops or the charger is unplugged",
                on,
                Some(Message::BatteryAware),
            ));
        }
        if let Some(battery) = &data.battery {
            rows.push(self.limit(ui, battery));
        }
        ui.group("Power mode", rows)
    }

    fn limit<'a>(&self, ui: Ui<'a>, battery: &Battery) -> Element<'a, Message> {
        let (Some(path), Some(limit)) = (&battery.limit_path, battery.limit) else {
            return ui.toggle(
                "Charge limit",
                "This battery's firmware doesn't offer one",
                false,
                None::<fn(bool) -> Message>,
            );
        };
        let value = self.limit.unwrap_or(f32::from(limit));
        ui.slide(
            "Charge limit",
            50.0..=100.0,
            5.0,
            value,
            format!("{value:.0}%"),
            Message::Limit,
            Message::SaveLimit(path.clone(), limit),
        )
    }

    fn away<'a>(&self, ui: Ui<'a>, data: &Data) -> Element<'a, Message> {
        let mut rows = match &data.idle {
            Some(text) => idle_rows(ui, text, Message::Idle),
            None => vec![ui.cell(ui.dim("Install hypridle to act when you step away"))],
        };
        if data.has_lid {
            let pick = ui.choose(&power::ACTIONS, &data.logind.lid, Message::Lid);
            rows.push(ui.row("Close the lid", "", pick));
        }
        let pick = ui.choose(&power::ACTIONS, &data.logind.power_key, Message::PowerKey);
        rows.push(ui.row("Press the power button", "", pick));
        ui.group("When you step away", rows)
    }

    fn logind(&self, edit: impl FnOnce(&mut Logind)) -> Effect<Message> {
        let Some(data) = &self.data else { return Effect::none() };
        let mut after = data.logind.clone();
        edit(&mut after);
        Effect::change(
            Change::new(power::set_logind(&after)).undo(power::set_logind(&data.logind)),
        )
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(load(), |data| Message::Loaded(Box::new(data)))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(data) => {
                self.data = Some(*data);
                self.limit = None;
                Effect::none()
            }
            Message::Profile(name) => match &self.data {
                Some(data) => Effect::change(power::set_profile(&data.profiles.active, &name)),
                None => Effect::none(),
            },
            Message::BatteryAware(on) => Effect::change(power::set_battery_aware(on)),
            Message::Limit(value) => {
                self.limit = Some(value);
                Effect::none()
            }
            Message::SaveLimit(path, before) => match self.limit {
                Some(value) => Effect::change(power::set_limit(&path, before, value as u8)),
                None => Effect::none(),
            },
            Message::Idle(stage, seconds) => match self.data.as_ref().and_then(|d| d.idle.as_ref())
            {
                Some(text) => Effect::change(idle::set(text, stage, seconds)),
                None => Effect::none(),
            },
            Message::Lid(action) => self.logind(|state| action.clone_into(&mut state.lid)),
            Message::PowerKey(action) => {
                self.logind(|state| action.clone_into(&mut state.power_key))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        const TITLE: &str = "Power and battery";
        let Some(data) = &self.data else {
            return ui.panel(TITLE, "", None, vec![ui.loading()]);
        };
        let mut body = Vec::new();
        if let Some(battery) = &data.battery {
            let health = format!("{:.0}% of original capacity left", battery.health() * 100.0);
            let cycles = battery.cycles.map_or(String::new(), |n| format!("{n} charge cycles"));
            let header = row![
                container(ui.percentage(battery.percent)).width(iced::Fill),
                column![ui.bold(health), ui.small(cycles)].align_x(iced::Right),
            ]
            .align_y(iced::Bottom);
            body.push(ui.card(column![header, home::gauge(ui, battery)].spacing(14)));
        }
        body.push(ui.columns(self.profiles(ui, data), self.away(ui, data)));
        let subtitle = data.battery.as_ref().map(Battery::summary).unwrap_or_default();
        ui.panel(TITLE, subtitle, None, body)
    }

    fn summary(&self) -> String {
        let Some(data) = &self.data else { return String::new() };
        match &data.battery {
            Some(battery) if battery.charging() => format!("Charging, {:.0}%", battery.percent),
            Some(battery) => format!("{:.0}%", battery.percent),
            None => describe(&data.profiles.active).1.to_owned(),
        }
    }

    fn watch(&self) -> Watch {
        Watch::Every(Duration::from_secs(5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::Op;

    #[test]
    fn delays_include_an_unusual_current_value() {
        assert!(delays(Some(95)).contains(&Delay(Some(95))));
        assert_eq!(Delay(Some(95)).to_string(), "After 95 s");
        assert_eq!(Delay(Some(7200)).to_string(), "After 2 h");
        assert_eq!(delays(None).last(), Some(&Delay(None)));
    }

    #[test]
    fn set_logind_keeps_both_actions_together() {
        let Op::Run(commands) = power::set_logind(&Logind {
            lid: "ignore".into(),
            power_key: "suspend".into(),
        }) else {
            panic!("expected a command")
        };
        assert_eq!(commands.len(), 1);
    }
}
