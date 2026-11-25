use iced::Task;
use iced::widget::{column, row, space};

use super::{Effect, Panel, Watch, quietly};
use crate::icon::Icon;
use crate::sys::network::{self, AccessPoint, Auth, Connection, Join, Network, Security};
use crate::sys::{Change, Result, rfkill};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    network: Option<Result<Network>>,
    selected: Option<String>,
    password: String,
    identity: String,
    skip_certificate: bool,
    scanning: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Result<Network>>),
    Wifi(bool),
    Airplane(bool),
    Scan,
    Scanned,
    Select(String),
    Password(String),
    Identity(String),
    CheckCertificate(bool),
    Join(String, Security),
    Up(String),
    Down(String),
    Forget(String),
    Autoconnect(String, bool),
}

impl State {
    fn network(&self) -> Option<&Network> {
        self.network.as_ref()?.as_ref().ok()
    }

    fn join(&mut self, ssid: String, security: Security) -> Effect<Message> {
        let Some(device) = self.network().and_then(Network::wifi_device) else {
            return Effect::none();
        };
        let device = device.name.clone();
        let password = std::mem::take(&mut self.password);
        let auth = match security {
            Security::Psk => Auth::Psk(password),
            Security::Sae => Auth::Sae(password),
            Security::Enterprise => Auth::Enterprise {
                identity: self.identity.clone(),
                password,
                check_certificate: !self.skip_certificate,
            },
            Security::Open | Security::Legacy => Auth::Open,
        };
        Effect::run(crate::sys::Op::Join(Join { device, ssid, auth, hotspot: false }))
    }

    fn can_join(&self, security: Security) -> bool {
        match security {
            Security::Open => true,
            // The limits are part of the WPA standard.
            Security::Psk => (8..=63).contains(&self.password.len()),
            Security::Sae => !self.password.is_empty(),
            Security::Enterprise => !self.identity.is_empty() && !self.password.is_empty(),
            Security::Legacy => false,
        }
    }

    fn access_point<'a>(
        &'a self,
        ui: Ui<'a>,
        ap: &'a AccessPoint,
        known: Option<&'a Connection>,
    ) -> Element<'a, Message> {
        let tone = if ap.active { Tone::Accent } else { Tone::Mute };
        let icon = match ap.signal {
            0..=34 => Icon::WifiLow,
            35..=64 => Icon::WifiMid,
            _ => Icon::Wifi,
        };
        let state = match (ap.active, known) {
            (true, _) => "Connected",
            (false, Some(_)) => "Saved",
            (false, None) => ap.security.label(),
        };
        let hint = if ap.five_ghz { format!("{state}, 5 GHz") } else { state.to_owned() };
        let row = ui.link(icon, tone, ap.ssid.as_str(), hint, Message::Select(ap.ssid.clone()));
        if self.selected.as_deref() != Some(ap.ssid.as_str()) {
            return row;
        }
        column![row, ui.cell(self.details(ui, ap, known))].into()
    }

    fn details<'a>(
        &'a self,
        ui: Ui<'a>,
        ap: &'a AccessPoint,
        known: Option<&'a Connection>,
    ) -> Element<'a, Message> {
        if let Some(connection) = known {
            let uuid = connection.uuid.clone();
            let connect = if ap.active {
                ui.button("Disconnect", Kind::Secondary, Some(Message::Down(uuid.clone())))
            } else {
                ui.button("Connect", Kind::Primary, Some(Message::Up(uuid.clone())))
            };
            let forget = ui.button("Forget", Kind::Danger, Some(Message::Forget(uuid.clone())));
            let auto = row![
                ui.switch(
                    connection.autoconnect,
                    Some(move |on| Message::Autoconnect(uuid.clone(), on))
                ),
                ui.text("Connect automatically"),
            ]
            .spacing(10)
            .align_y(iced::Center);
            return ui.actions(vec![connect, forget, auto.into()]);
        }
        let join = || Message::Join(ap.ssid.clone(), ap.security);
        let button =
            ui.button("Connect", Kind::Primary, self.can_join(ap.security).then(join));
        let password = ui
            .field("Password", &self.password, Message::Password)
            .secure(true)
            .on_submit_maybe(self.can_join(ap.security).then(join));
        match ap.security {
            Security::Open => column![
                ui.small("Anyone nearby can read unencrypted traffic on this network."),
                ui.actions(vec![button]),
            ]
            .spacing(10)
            .into(),
            Security::Psk | Security::Sae => {
                row![password, button].spacing(8).align_y(iced::Center).into()
            }
            Security::Enterprise => column![
                ui.field("User name", &self.identity, Message::Identity),
                password,
                row![
                    ui.switch(!self.skip_certificate, Some(Message::CheckCertificate)),
                    ui.text("Check the server's certificate"),
                ]
                .spacing(10)
                .align_y(iced::Center),
                ui.small(
                    "Without the check, a fake access point can collect your password. \
                     Turn it off only if your organisation says so."
                ),
                ui.actions(vec![button]),
            ]
            .spacing(10)
            .into(),
            Security::Legacy => ui
                .small(
                    "This network uses WEP, which can be broken in minutes. \
                     Connect with nmcli if you must.",
                )
                .into(),
        }
    }

    fn wireless<'a>(&'a self, ui: Ui<'a>, network: &'a Network) -> Element<'a, Message> {
        if !network.wifi_present {
            return ui.empty(
                Icon::Wifi,
                "No wireless adapter",
                "Plug one in, or check that its firmware is installed under Drivers.",
                None,
            );
        }
        let mut rows = vec![ui.toggle("Wi-Fi", "", network.wifi_enabled, Some(Message::Wifi))];
        if network.wifi_enabled {
            rows.extend(
                network
                    .access_points
                    .iter()
                    .map(|ap| self.access_point(ui, ap, network.known(&ap.ssid))),
            );
        }
        ui.group("", rows)
    }

    fn details_card<'a>(&'a self, ui: Ui<'a>, network: &'a Network) -> Element<'a, Message> {
        let mut rows = Vec::new();
        if let Some(address) = &network.address {
            rows.push(ui.value("Address", address.ip.as_str()));
            rows.push(ui.value("Gateway", address.gateway.as_str()));
            rows.push(ui.value("DNS", address.dns.join(", ")));
            rows.push(ui.value("Hardware address", address.hardware.as_str()));
        }
        if let Some(wired) = network.wired_device() {
            let state = match wired.state.as_str() {
                "connected" => wired.connection.as_str(),
                "unavailable" => "No cable",
                state => state,
            };
            rows.push(ui.value("Ethernet", state));
        }
        ui.group("This connection", rows)
    }

    fn saved<'a>(&'a self, ui: Ui<'a>, network: &'a Network) -> Element<'a, Message> {
        let rows: Vec<_> = network
            .connections
            .iter()
            .filter(|c| c.is_wifi() && c.name != network::HOTSPOT)
            .filter(|c| !network.access_points.iter().any(|ap| ap.ssid == c.name))
            .map(|c| {
                let forget = Message::Forget(c.uuid.clone());
                ui.row(
                    c.name.as_str(),
                    "Out of range",
                    ui.button("Forget", Kind::Danger, Some(forget)),
                )
            })
            .collect();
        if rows.is_empty() { space().into() } else { ui.group("Saved networks", rows) }
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
            Message::Wifi(on) => {
                Effect::change(Change::new(network::set_wifi(on)).undo(network::set_wifi(!on)))
            }
            Message::Airplane(on) => match self.network() {
                Some(network) => Effect::change(rfkill::set_airplane(&network.radios, on)),
                None => Effect::none(),
            },
            Message::Scan => {
                self.scanning = true;
                Effect::task(quietly(network::rescan(), Message::Scanned))
            }
            Message::Scanned => {
                self.scanning = false;
                Effect::task(self.load())
            }
            Message::Select(ssid) => {
                self.password.clear();
                self.selected = (self.selected.as_ref() != Some(&ssid)).then_some(ssid);
                Effect::none()
            }
            Message::Password(password) => {
                self.password = password;
                Effect::none()
            }
            Message::Identity(identity) => {
                self.identity = identity;
                Effect::none()
            }
            Message::CheckCertificate(on) => {
                self.skip_certificate = !on;
                Effect::none()
            }
            Message::Join(ssid, security) => self.join(ssid, security),
            Message::Up(uuid) => Effect::run(network::up(&uuid)),
            Message::Down(uuid) => {
                Effect::change(Change::new(network::down(&uuid)).undo(network::up(&uuid)))
            }
            Message::Forget(uuid) => Effect::run(network::forget(&uuid)),
            Message::Autoconnect(uuid, on) => Effect::change(
                Change::new(network::set_autoconnect(&uuid, on))
                    .undo(network::set_autoconnect(&uuid, !on)),
            ),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let network = match &self.network {
            None => return ui.panel("Network", "", None, vec![ui.loading()]),
            Some(Err(error)) => {
                return ui.panel("Network", "", None, vec![ui.failure(error.to_string())]);
            }
            Some(Ok(network)) => network,
        };
        let scan = ui.button(
            if self.scanning { "Scanning" } else { "Scan" },
            Kind::Secondary,
            (network.wifi_enabled && !self.scanning).then_some(Message::Scan),
        );
        let airplane = ui.group(
            "",
            vec![ui.toggle(
                "Airplane mode",
                "Turns off Wi-Fi, Bluetooth and mobile data",
                rfkill::airplane(&network.radios),
                (!network.radios.is_empty()).then_some(Message::Airplane),
            )],
        );
        ui.panel(
            "Network",
            self.summary(),
            Some(scan),
            vec![ui.split(
                3,
                self.wireless(ui, network),
                2,
                ui.stack(vec![
                    self.details_card(ui, network),
                    airplane,
                    self.saved(ui, network),
                ]),
            )],
        )
    }

    fn summary(&self) -> String {
        let Some(network) = self.network() else { return String::new() };
        let wired = network.wired_device().filter(|device| device.state == "connected");
        match (wired, network.active_wifi()) {
            (Some(_), _) => "Wired".to_owned(),
            (None, Some(ap)) => ap.ssid.clone(),
            (None, None) if network.wifi_enabled => "Not connected".to_owned(),
            (None, None) => "Wi-Fi off".to_owned(),
        }
    }

    fn watch(&self) -> Watch {
        Watch::Command(&["nmcli", "monitor"])
    }
}
