use iced::Task;

use super::{Effect, Panel};
use crate::sys::services::{self, Sysctl, TWEAKS, Zram};
use crate::sys::{self};
use crate::ui::{Element, Ui};

#[derive(Debug, Clone, Default)]
pub struct Data {
    /// One entry per [`TWEAKS`], `None` where the unit is not installed.
    states: Vec<Option<bool>>,
    sysctls: Vec<Sysctl>,
    zram: Option<Zram>,
}

#[derive(Default)]
pub struct State {
    data: Option<Data>,
    dragging: Option<(usize, f32)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Data),
    Toggle(usize, bool),
    Drag(usize, f32),
    Release,
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(services::tweaks(), |states| {
            Message::Loaded(Data {
                states,
                sysctls: services::sysctls(),
                zram: services::zram(),
            })
        })
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(data) => {
                self.data = Some(data);
                Effect::none()
            }
            Message::Toggle(index, on) => {
                let tweak = &TWEAKS[index];
                Effect::change(services::set_now(tweak.scope, tweak.unit, on))
            }
            Message::Drag(index, value) => {
                self.dragging = Some((index, value));
                Effect::none()
            }
            Message::Release => {
                let (Some((index, value)), Some(data)) = (self.dragging.take(), &self.data) else {
                    return Effect::none();
                };
                let sysctl = &data.sysctls[index];
                if (sysctl.value - value).abs() < 0.5 {
                    return Effect::none();
                }
                Effect::change(services::set_sysctl(sysctl.key, sysctl.value, value))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(data) = &self.data else {
            return ui.panel("Tweaks", "", None, vec![ui.loading()]);
        };
        let toggles = TWEAKS
            .iter()
            .zip(&data.states)
            .enumerate()
            .filter_map(|(index, (tweak, state))| {
                let on = (*state)?;
                Some(ui.toggle(
                    tweak.label,
                    tweak.hint,
                    on,
                    Some(move |on| Message::Toggle(index, on)),
                ))
            })
            .collect();
        let mut memory: Vec<_> = data
            .sysctls
            .iter()
            .enumerate()
            .map(|(index, sysctl)| {
                let value = match self.dragging {
                    Some((dragged, value)) if dragged == index => value,
                    _ => sysctl.value,
                };
                let slider = ui.slide(
                    sysctl.label,
                    0.0..=sysctl.max,
                    if sysctl.max > 1.0 { 5.0 } else { 1.0 },
                    value,
                    format!("{value:.0}"),
                    move |value| Message::Drag(index, value),
                    Message::Release,
                );
                iced::widget::column![slider, ui.cell(ui.small(sysctl.hint))].into()
            })
            .collect();
        if let Some(zram) = &data.zram {
            let detail = format!("{}, {}", sys::bytes(zram.size), zram.algorithm);
            memory.push(ui.value("Compressed swap (zram)", detail));
        }
        ui.panel(
            "Tweaks",
            "Only services that are installed are listed",
            None,
            vec![ui.columns(ui.group("Background services", toggles), ui.group("Kernel", memory))],
        )
    }

    fn summary(&self) -> String {
        match &self.data {
            Some(data) => {
                let on = data.states.iter().filter(|state| **state == Some(true)).count();
                format!("{on} on")
            }
            None => String::new(),
        }
    }
}
