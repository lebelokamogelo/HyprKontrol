use iced::Task;
use iced::widget::column;

use super::{Effect, Panel, Watch};
use crate::icon::Icon;
use crate::sys::network::{self, Auth, Join, Network};
use crate::sys::{Op, Result};
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    network: Option<Result<Network>>,
    name: String,
    password: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Result<Network>>),
    Name(String),
    Password(String),
    Start,
    Stop(String),
}

impl State {
    fn ready(&self) -> bool {
        (1..=32).contains(&self.name.trim().len()) && (8..=63).contains(&self.password.len())
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(network::load(), |n| Message::Loaded(Box::new(n)))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(network) => {
                self.network = Some(*network);
                Effect::none()
            }
            Message::Name(name) => {
                self.name = name;
                Effect::none()
            }
            Message::Password(password) => {
                self.password = password;
                Effect::none()
            }
            Message::Start => {
                let Some(Ok(network)) = &self.network else { return Effect::none() };
                let Some(device) = network.wifi_device() else { return Effect::none() };
                Effect::run(Op::Join(Join {
                    device: device.name.clone(),
                    ssid: self.name.trim().to_owned(),
                    auth: Auth::Psk(self.password.clone()),
                    hotspot: true,
                }))
            }
            // The profile holds a password, so it is removed and not just stopped.
            Message::Stop(uuid) => Effect::run(network::forget(&uuid)),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let network = match &self.network {
            None => return ui.panel("Hotspot", "", None, vec![ui.loading()]),
            Some(Err(error)) => {
                return ui.panel("Hotspot", "", None, vec![ui.failure(error.to_string())]);
            }
            Some(Ok(network)) => network,
        };
        let subtitle = "Share this computer's connection over Wi-Fi";
        if network.wifi_device().is_none() {
            let empty = ui.empty(
                Icon::Hotspot,
                "No wireless adapter",
                "A hotspot needs a Wi-Fi adapter that can act as an access point.",
                None,
            );
            return ui.panel("Hotspot", subtitle, None, vec![empty]);
        }
        if let Some(active) = network.hotspot() {
            let stop =
                ui.button("Stop hotspot", Kind::Danger, Some(Message::Stop(active.uuid.clone())));
            let running = ui.empty(
                Icon::Hotspot,
                "The hotspot is running",
                "Other devices can connect until you stop it.",
                Some(stop),
            );
            return ui.panel("Hotspot", subtitle, None, vec![running]);
        }
        let form = column![
            ui.field("Network name", &self.name, Message::Name),
            ui.field("Password, 8 to 63 characters", &self.password, Message::Password)
                .secure(true),
            ui.button("Start hotspot", Kind::Primary, self.ready().then_some(Message::Start)),
        ]
        .spacing(10);
        let mut body = vec![ui.group("New hotspot", vec![ui.cell(form)])];
        if network.active_wifi().is_some() {
            body.push(ui.notice(
                Icon::Wifi,
                "Most adapters cannot do both at once. Starting a hotspot will disconnect \
                 you from Wi-Fi.",
            ));
        }
        ui.panel("Hotspot", subtitle, None, body)
    }

    fn summary(&self) -> String {
        match &self.network {
            Some(Ok(network)) if network.hotspot().is_some() => "Running".to_owned(),
            Some(Ok(_)) => "Off".to_owned(),
            _ => String::new(),
        }
    }

    fn watch(&self) -> Watch {
        Watch::Command(&["nmcli", "monitor"])
    }
}
