use std::time::Duration;

use iced::widget::row;
use iced::{Center, Task};

use super::{Effect, Panel, Watch};
use crate::sys::region::{self, Clock};
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    clock: Option<Clock>,
    zones: Vec<String>,
    query: String,
    manual: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Clock),
    Zones(Vec<String>),
    Query(String),
    Zone(String),
    Ntp(bool),
    LocalRtc(bool),
    Manual(String),
    SetTime,
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        let clock = Task::perform(region::clock(), Message::Loaded);
        if self.zones.is_empty() {
            return Task::batch([clock, Task::perform(region::zones(), Message::Zones)]);
        }
        clock
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(clock) => {
                self.clock = Some(clock);
                Effect::none()
            }
            Message::Zones(zones) => {
                self.zones = zones;
                Effect::none()
            }
            Message::Query(query) => {
                self.query = query;
                Effect::none()
            }
            Message::Zone(zone) => match &self.clock {
                Some(clock) => Effect::change(region::set_zone(&clock.zone, &zone)),
                None => Effect::none(),
            },
            Message::Ntp(on) => Effect::change(region::set_ntp(on)),
            Message::LocalRtc(on) => Effect::change(region::set_local_rtc(on)),
            Message::Manual(text) => {
                self.manual = text;
                Effect::none()
            }
            Message::SetTime => {
                let when = std::mem::take(&mut self.manual);
                Effect::change(region::set_time(when.trim()))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        const TITLE: &str = "Date and time";
        let Some(clock) = &self.clock else {
            return ui.panel(TITLE, "", None, vec![ui.loading()]);
        };
        let hint = match (clock.ntp, clock.synchronized) {
            (true, true) => "In sync with the internet",
            (true, false) => "Waiting for a time server",
            (false, _) => "",
        };
        let mut rows = vec![ui.toggle(
            "Set the time automatically",
            hint,
            clock.ntp,
            clock.can_ntp.then_some(Message::Ntp),
        )];
        if !clock.ntp {
            let ready = self.manual.trim().len() >= 16;
            let field = row![
                ui.field("2026-01-31 14:30", &self.manual, Message::Manual)
                    .on_submit_maybe(ready.then_some(Message::SetTime)),
                ui.button("Set", Kind::Primary, ready.then_some(Message::SetTime)),
            ]
            .spacing(8)
            .align_y(Center);
            rows.push(ui.cell(field));
        }
        rows.push(ui.toggle(
            "Hardware clock in local time",
            "Only needed when sharing this computer with Windows",
            clock.local_rtc,
            Some(Message::LocalRtc),
        ));
        let query = self.query.to_lowercase().replace(' ', "_");
        let zones = self
            .zones
            .iter()
            .filter(move |zone| query.is_empty() || zone.to_lowercase().contains(&query))
            .map(|zone| {
                (zone.replace('_', " "), *zone == clock.zone, Message::Zone(zone.clone()))
            });
        let zone = ui.group(
            format!("Time zone: {}", clock.zone.replace('_', " ")),
            vec![ui.finder(&self.query, Message::Query, zones)],
        );
        ui.panel(
            TITLE,
            clock.now.as_str(),
            None,
            vec![ui.columns(ui.group("", rows), zone)],
        )
    }

    fn summary(&self) -> String {
        self.clock.as_ref().map(|clock| clock.zone.replace('_', " ")).unwrap_or_default()
    }

    fn watch(&self) -> Watch {
        Watch::Every(Duration::from_secs(20))
    }
}
