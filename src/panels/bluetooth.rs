use std::time::Duration;

use iced::Task;

use super::{Effect, Panel, Watch, quietly};
use crate::icon::Icon;
use crate::sys::bluetooth::{self, Block, Bluetooth, Device};
use crate::sys::services::{self, Scope};
use crate::sys::{Change, Op, Result};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

const SCAN_SECONDS: u32 = 15;

#[derive(Default)]
pub struct State {
    bluetooth: Option<Result<Bluetooth>>,
    scanning: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<Bluetooth>),
    Power(bool),
    Discoverable(bool),
    Scan,
    Scanned,
    Connect(String),
    Disconnect(String),
    Pair(String),
    Trust(String, bool),
    Remove(String),
    StartService,
}

impl State {
    fn known(&self) -> Option<&Bluetooth> {
        self.bluetooth.as_ref()?.as_ref().ok()
    }
}

fn device_icon(device: &Device) -> Icon {
    match device.icon.as_str() {
        icon if icon.contains("head") || icon.contains("audio") => Icon::Headphones,
        icon if icon.contains("mouse") => Icon::Mouse,
        icon if icon.contains("keyboard") => Icon::Keyboard,
        icon if icon.contains("printer") => Icon::Printer,
        icon if icon.contains("computer") => Icon::Monitor,
        _ => Icon::Bluetooth,
    }
}

fn paired<'a>(ui: Ui<'a>, device: &'a Device) -> Element<'a, Message> {
    let address = device.address.clone();
    let hint = match (device.connected, device.battery) {
        (true, Some(percent)) => format!("Connected, battery {percent}%"),
        (true, None) => "Connected".to_owned(),
        (false, _) => "Not connected".to_owned(),
    };
    let connect = if device.connected {
        ui.button("Disconnect", Kind::Secondary, Some(Message::Disconnect(address.clone())))
    } else {
        ui.button("Connect", Kind::Secondary, Some(Message::Connect(address.clone())))
    };
    let trust = address.clone();
    let controls = ui.actions(vec![
        connect,
        ui.switch(device.trusted, Some(move |on| Message::Trust(trust.clone(), on))),
        ui.icon_button(Icon::Close, Tone::Mute, Some(Message::Remove(address))),
    ]);
    let tone = if device.connected { Tone::Accent } else { Tone::Mute };
    ui.icon_row(device_icon(device), tone, device.name.as_str(), hint, controls)
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(bluetooth::load(), Message::Loaded)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(bluetooth) => {
                self.bluetooth = Some(bluetooth);
                Effect::none()
            }
            Message::Power(on) => {
                let block = self.known().map_or(Block::None, |bluetooth| bluetooth.block);
                Effect::change(bluetooth::set_power(on, block))
            }
            Message::Discoverable(on) => Effect::change(
                Change::new(bluetooth::discoverable(on)).undo(bluetooth::discoverable(!on)),
            ),
            Message::Scan => {
                self.scanning = true;
                Effect::task(quietly(bluetooth::scan(SCAN_SECONDS), Message::Scanned))
            }
            Message::Scanned => {
                self.scanning = false;
                Effect::task(self.load())
            }
            Message::Connect(address) => Effect::change(
                Change::new(bluetooth::connect(&address)).undo(bluetooth::disconnect(&address)),
            ),
            Message::Disconnect(address) => Effect::change(
                Change::new(bluetooth::disconnect(&address)).undo(bluetooth::connect(&address)),
            ),
            Message::Pair(address) => Effect::change(
                Change::new(Op::Run(bluetooth::pair(&address))).undo(bluetooth::remove(&address)),
            ),
            Message::Trust(address, on) => Effect::change(
                Change::new(bluetooth::trust(&address, on)).undo(bluetooth::trust(&address, !on)),
            ),
            Message::Remove(address) => Effect::run(bluetooth::remove(&address)),
            Message::StartService => {
                Effect::change(services::set_now(Scope::System, "bluetooth.service", true))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let bluetooth = match &self.bluetooth {
            None => return ui.panel("Bluetooth", "", None, vec![ui.loading()]),
            Some(Err(error)) => {
                return ui.panel("Bluetooth", "", None, vec![ui.failure(error.to_string())]);
            }
            Some(Ok(bluetooth)) => bluetooth,
        };
        let Some(on) = bluetooth.powered else {
            let start =
                ui.button("Start Bluetooth", Kind::Primary, Some(Message::StartService));
            let empty = ui.empty(
                Icon::Bluetooth,
                "Bluetooth is not available",
                "Either there is no adapter or the bluetooth service is stopped.",
                Some(start),
            );
            return ui.panel("Bluetooth", "", None, vec![empty]);
        };

        let scan = ui.button(
            if self.scanning { "Searching" } else { "Search for devices" },
            Kind::Secondary,
            (on && !self.scanning).then_some(Message::Scan),
        );
        let mut body = vec![ui.group(
            "",
            vec![
                ui.toggle(
                    "Bluetooth",
                    match bluetooth.block {
                        Block::None => "",
                        Block::Software => "Blocked by airplane mode or a hotkey. Turning it on lifts the block",
                        Block::Hardware => "Turned off by a switch on this computer or in its firmware",
                    },
                    on,
                    (bluetooth.block != Block::Hardware).then_some(Message::Power),
                ),
                ui.toggle(
                    "Visible to other devices",
                    format!("As \"{}\"", bluetooth.name),
                    bluetooth.discoverable,
                    on.then_some(Message::Discoverable),
                ),
            ],
        )];
        if on {
            let (known, found): (Vec<_>, Vec<_>) =
                bluetooth.devices.iter().partition(|device| device.paired);
            if known.is_empty() && found.is_empty() {
                body.push(ui.empty(
                    Icon::Bluetooth,
                    "No devices yet",
                    "Put a device in pairing mode, then search for it.",
                    None,
                ));
            }
            if !known.is_empty() {
                let rows = known.into_iter().map(|device| paired(ui, device)).collect();
                body.push(ui.group("Your devices. The switch lets one reconnect by itself.", rows));
            }
            if !found.is_empty() {
                let rows = found
                    .into_iter()
                    .map(|device| {
                        let pair = Message::Pair(device.address.clone());
                        ui.icon_row(
                            device_icon(device),
                            Tone::Mute,
                            device.name.as_str(),
                            device.address.as_str(),
                            ui.button("Pair", Kind::Secondary, Some(pair)),
                        )
                    })
                    .collect();
                body.push(ui.group("Nearby", rows));
            }
        }
        ui.panel("Bluetooth", self.summary(), Some(scan), body)
    }

    fn summary(&self) -> String {
        let Some(bluetooth) = self.known() else { return String::new() };
        let connected: Vec<_> = bluetooth.devices.iter().filter(|d| d.connected).collect();
        match (bluetooth.powered, connected.as_slice()) {
            (None, _) => "Not available".to_owned(),
            (Some(false), _) => "Off".to_owned(),
            (Some(true), []) => "On".to_owned(),
            (Some(true), [device]) => device.name.clone(),
            (Some(true), devices) => format!("{} connected", devices.len()),
        }
    }

    fn watch(&self) -> Watch {
        // bluetoothctl has no monitor mode that survives without a terminal.
        Watch::Every(Duration::from_secs(if self.scanning { 2 } else { 5 }))
    }
}
