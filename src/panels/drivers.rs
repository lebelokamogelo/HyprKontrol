use iced::Task;

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::packages::{self, Driver};
use crate::sys::{installed, probe};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    drivers: Option<Vec<Driver>>,
    gpus: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Vec<Driver>, Vec<String>),
    Install(String),
    Remove(String),
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(
            async {
                let (drivers, machine) = tokio::join!(packages::drivers(), probe::machine());
                (drivers, machine.gpus)
            },
            |(drivers, gpus)| Message::Loaded(drivers, gpus),
        )
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(drivers, gpus) => {
                self.drivers = Some(drivers);
                self.gpus = gpus;
                Effect::none()
            }
            Message::Install(profile) => Effect::session(packages::install_driver(&profile)),
            Message::Remove(profile) => Effect::session(packages::remove_driver(&profile)),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(drivers) = &self.drivers else {
            return ui.panel("Drivers", "", None, vec![ui.loading()]);
        };
        if !installed("chwd") {
            let empty = ui.empty(
                Icon::Gpu,
                "chwd is not installed",
                "CachyOS uses it to match drivers to your hardware.",
                None,
            );
            return ui.panel("Drivers", "", None, vec![empty]);
        }
        let mut body = Vec::new();
        if !self.gpus.is_empty() {
            let rows = self.gpus.iter().map(|gpu| ui.value("Graphics", gpu.as_str())).collect();
            body.push(ui.group("", rows));
        }
        let mut devices: Vec<&str> = drivers.iter().map(|driver| driver.device.as_str()).collect();
        devices.dedup();
        body.extend(devices.into_iter().map(|device| {
            let rows = drivers
                .iter()
                .filter(|driver| driver.device == device)
                .map(|driver| {
                    let profile = driver.profile.clone();
                    let control = if driver.installed {
                        ui.button("Remove", Kind::Danger, Some(Message::Remove(profile)))
                    } else {
                        ui.button("Install", Kind::Secondary, Some(Message::Install(profile)))
                    };
                    let (tone, hint) = if driver.installed {
                        (Tone::Accent, "In use")
                    } else {
                        (Tone::Mute, "")
                    };
                    ui.icon_row(Icon::Gpu, tone, driver.profile.as_str(), hint, control)
                })
                .collect();
            ui.group(device, rows)
        }));
        ui.panel(
            "Drivers",
            "Profiles that match your hardware",
            None,
            body,
        )
    }

    fn summary(&self) -> String {
        match &self.drivers {
            None => String::new(),
            Some(_) => self.gpus.first().cloned().unwrap_or_else(|| "Hardware".to_owned()),
        }
    }
}
