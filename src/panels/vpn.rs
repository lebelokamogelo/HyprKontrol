use std::collections::HashMap;
use std::path::PathBuf;

use iced::widget::{column, row};
use iced::{Center, Task};

use super::{Effect, Panel, Request, Watch};
use crate::icon::Icon;
use crate::sys::network::{self, Connection, Network};
use crate::sys::vpn::{self, Import, Login};
use crate::sys::{Change, Op, Result};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    network: Option<Result<Network>>,
    logins: HashMap<String, Login>,
    /// The profile whose password is being entered.
    asking: Option<String>,
    username: String,
    password: String,
    path: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Result<Network>>, HashMap<String, Login>),
    Set(String, bool),
    Autoconnect(String, bool),
    Remove(String),
    Username(String),
    Password(String),
    Connect,
    Cancel,
    Path(String),
    Import,
}

async fn load() -> (Result<Network>, HashMap<String, Login>) {
    let network = network::load().await;
    let mut logins = HashMap::new();
    if let Ok(network) = &network {
        for connection in network.connections.iter().filter(|c| c.kind == "vpn") {
            logins.insert(connection.uuid.clone(), vpn::login(&connection.uuid).await);
        }
    }
    (network, logins)
}

impl State {
    fn connections(&self) -> impl Iterator<Item = &Connection> {
        let connections = match &self.network {
            Some(Ok(network)) => network.connections.as_slice(),
            _ => &[],
        };
        connections.iter().filter(|connection| connection.is_vpn())
    }

    fn set(&mut self, uuid: String, on: bool) -> Effect<Message> {
        let login = self.logins.get(&uuid).cloned().unwrap_or_default();
        if on && login.needs_password {
            self.username = login.username;
            self.password.clear();
            self.asking = Some(uuid);
            return Effect::none();
        }
        let set = |on| if on { network::up(&uuid) } else { network::down(&uuid) };
        let change = Change::new(set(on));
        // Reconnecting would need the password again, so there is no Undo.
        Effect::change(if login.needs_password { change } else { change.undo(set(!on)) })
    }

    fn connect(&mut self) -> Effect<Message> {
        let Some(uuid) = self.asking.take() else { return Effect::none() };
        let before = self.logins.get(&uuid).cloned().unwrap_or_default();
        let password = std::mem::take(&mut self.password);
        let commands = vpn::connect(&uuid, &before, self.username.trim(), &password);
        Effect::change(Change::new(Op::Run(commands)).undo(network::down(&uuid)))
    }

    fn import(&self) -> Effect<Message> {
        let path = match self.path.trim().strip_prefix("~/") {
            Some(rest) => crate::sys::home().join(rest),
            None => PathBuf::from(self.path.trim()),
        };
        match Import::new(path) {
            Ok(import) => Effect::run(Op::ImportVpn(import)),
            Err(error) => Effect::request(Request::Report(error)),
        }
    }

    fn entry<'a>(&'a self, ui: Ui<'a>, connection: &'a Connection) -> Element<'a, Message> {
        let kind = if connection.kind == "wireguard" { "WireGuard" } else { "OpenVPN" };
        let uuid = &connection.uuid;
        let set = uuid.clone();
        let automatic = ui.button(
            if connection.autoconnect { "Automatic" } else { "Manual" },
            Kind::Secondary,
            Some(Message::Autoconnect(uuid.clone(), !connection.autoconnect)),
        );
        let controls = ui.actions(vec![
            automatic,
            ui.icon_button(Icon::Close, Tone::Mute, Some(Message::Remove(uuid.clone()))),
            ui.switch(connection.active, Some(move |on| Message::Set(set.clone(), on))),
        ]);
        let tone = if connection.active { Tone::Accent } else { Tone::Mute };
        let line = ui.icon_row(Icon::Vpn, tone, connection.name.as_str(), kind, controls);
        if self.asking.as_ref() == Some(uuid) {
            column![line, ui.cell(self.form(ui))].into()
        } else {
            line
        }
    }

    fn form<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let ready = vpn::valid_username(self.username.trim()) && !self.password.is_empty();
        let username = ui.field("User name", &self.username, Message::Username);
        let password = ui
            .field("Password", &self.password, Message::Password)
            .secure(true)
            .on_submit_maybe(ready.then_some(Message::Connect));
        let fields: Element<'a, Message> = if ui.content < 520.0 {
            column![username, password].spacing(8).into()
        } else {
            row![username, password].spacing(8).into()
        };
        column![
            fields,
            row![
                ui.button("Connect", Kind::Primary, ready.then_some(Message::Connect)),
                ui.button("Cancel", Kind::Link, Some(Message::Cancel)),
                ui.small("The password is used once and not saved."),
            ]
            .spacing(10)
            .align_y(Center),
        ]
        .spacing(10)
        .into()
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(load(), |(network, logins)| Message::Loaded(Box::new(network), logins))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(network, logins) => {
                let before = self.connections().count();
                self.network = Some(*network);
                self.logins = logins;
                // The path is kept until it has led to a new profile, so that a
                // failed import can be corrected instead of typed again.
                if self.connections().count() > before {
                    self.path.clear();
                }
                Effect::none()
            }
            Message::Set(uuid, on) => self.set(uuid, on),
            Message::Autoconnect(uuid, on) => Effect::change(
                Change::new(network::set_autoconnect(&uuid, on))
                    .undo(network::set_autoconnect(&uuid, !on)),
            ),
            Message::Remove(uuid) => Effect::run(network::forget(&uuid)),
            Message::Username(username) => {
                self.username = username;
                Effect::none()
            }
            Message::Password(password) => {
                self.password = password;
                Effect::none()
            }
            Message::Connect => self.connect(),
            Message::Cancel => {
                self.asking = None;
                self.password.clear();
                Effect::none()
            }
            Message::Path(path) => {
                self.path = path;
                Effect::none()
            }
            Message::Import => self.import(),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        match &self.network {
            None => return ui.panel("VPN", "", None, vec![ui.loading()]),
            Some(Err(error)) => {
                return ui.panel("VPN", "", None, vec![ui.failure(error.to_string())]);
            }
            Some(Ok(_)) => {}
        }
        let mut body = Vec::new();
        let rows: Vec<_> = self.connections().map(|c| self.entry(ui, c)).collect();
        body.push(if rows.is_empty() {
            ui.empty(
                Icon::Vpn,
                "No VPN connections",
                "Import a WireGuard or OpenVPN file from your provider below.",
                None,
            )
        } else {
            ui.group("", rows)
        });
        let can_import = !self.path.trim().is_empty();
        let import = row![
            ui.field("Path to a .conf or .ovpn file", &self.path, Message::Path)
                .on_submit_maybe(can_import.then_some(Message::Import)),
            ui.button("Import", Kind::Primary, can_import.then_some(Message::Import)),
        ]
        .spacing(8)
        .align_y(Center);
        body.push(ui.group("Add a connection", vec![ui.cell(import)]));
        ui.panel("VPN", self.summary(), None, body)
    }

    fn summary(&self) -> String {
        if !matches!(self.network, Some(Ok(_))) {
            return String::new();
        }
        match self.connections().find(|c| c.active) {
            Some(connection) => connection.name.clone(),
            None if self.connections().next().is_some() => "Not connected".to_owned(),
            None => "None set up".to_owned(),
        }
    }

    fn watch(&self) -> Watch {
        Watch::Command(&["nmcli", "monitor"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(count: usize) -> Box<Result<Network>> {
        let connection = |n: usize| Connection {
            uuid: n.to_string(),
            name: format!("vpn{n}"),
            kind: "vpn".into(),
            autoconnect: false,
            active: false,
        };
        Box::new(Ok(Network {
            connections: (0..count).map(connection).collect(),
            ..Network::default()
        }))
    }

    #[test]
    fn the_path_survives_a_failed_import() {
        let mut state = State::default();
        let _ = state.update(Message::Loaded(network(0), HashMap::new()));
        let _ = state.update(Message::Path("~/a.ovpn".into()));
        let _ = state.update(Message::Loaded(network(0), HashMap::new()));
        assert_eq!(state.path, "~/a.ovpn");
        let _ = state.update(Message::Loaded(network(1), HashMap::new()));
        assert!(state.path.is_empty());
    }

    #[test]
    fn a_profile_with_a_login_asks_before_connecting() {
        let mut state = State::default();
        let login = Login { username: "ada".into(), needs_password: true };
        let logins = HashMap::from([("0".to_owned(), login)]);
        let _ = state.update(Message::Loaded(network(1), logins));
        assert!(state.update(Message::Set("0".into(), true)).change.is_none());
        assert_eq!(state.username, "ada");
        let _ = state.update(Message::Password("secret".into()));
        let change = state.update(Message::Connect).change.unwrap();
        assert!(!change.apply.describe().contains("secret"));
        assert!(state.password.is_empty(), "the password must not linger");
    }
}
