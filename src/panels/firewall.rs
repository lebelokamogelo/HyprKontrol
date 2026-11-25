use iced::widget::{column, row};
use iced::{Center, Task};

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::ufw::{self, Firewall, Rule};
use crate::sys::packages;
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    firewall: Option<Firewall>,
    port: String,
    protocol: usize,
    action: usize,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Firewall),
    Enable(bool),
    Incoming(&'static str),
    Outgoing(&'static str),
    Port(String),
    Protocol(usize),
    Action(usize),
    Add,
    Delete(Rule),
    Install,
}

const POLICIES: [(&str, &str); 3] = [("allow", "Allow"), ("deny", "Block"), ("reject", "Refuse")];
const PROTOCOLS: [&str; 3] = ["any", "tcp", "udp"];
const ACTIONS: [&str; 2] = ["allow", "deny"];

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::future(async { Message::Loaded(ufw::load()) })
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        let policy = |direction, get: fn(&Firewall) -> &str, after: &str| match &self.firewall {
            Some(firewall) => Effect::change(ufw::set_policy(direction, get(firewall), after)),
            None => Effect::none(),
        };
        match message {
            Message::Loaded(firewall) => {
                self.firewall = Some(firewall);
                Effect::none()
            }
            Message::Enable(on) => Effect::change(ufw::set_enabled(on)),
            Message::Incoming(after) => policy("incoming", |f| &f.incoming, after),
            Message::Outgoing(after) => policy("outgoing", |f| &f.outgoing, after),
            Message::Port(port) => {
                self.port = port;
                Effect::none()
            }
            Message::Protocol(index) => {
                self.protocol = index;
                Effect::none()
            }
            Message::Action(index) => {
                self.action = index;
                Effect::none()
            }
            Message::Add => {
                let port = std::mem::take(&mut self.port);
                // ufw needs a protocol for port lists and ranges.
                let several = port.contains([',', ':']);
                let protocol = match PROTOCOLS[self.protocol] {
                    "any" if several => "tcp",
                    protocol => protocol,
                };
                Effect::change(ufw::add(&Rule {
                    action: ACTIONS[self.action].to_owned(),
                    protocol: protocol.to_owned(),
                    port,
                    source: "any".to_owned(),
                    incoming: true,
                }))
            }
            Message::Delete(rule) => Effect::change(ufw::delete(&rule)),
            Message::Install => Effect::session(packages::install(&["ufw"])),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(firewall) = &self.firewall else {
            return ui.panel("Firewall", "", None, vec![ui.loading()]);
        };
        if !firewall.installed {
            let empty = ui.empty(
                Icon::Shield,
                "No firewall is installed",
                "CachyOS uses ufw.",
                Some(ui.button("Install ufw", Kind::Primary, Some(Message::Install))),
            );
            return ui.panel("Firewall", "", None, vec![empty]);
        }
        let policy = ui.group(
            "",
            vec![
                ui.toggle(
                    "Firewall",
                    "Filters connections from the network",
                    firewall.enabled,
                    Some(Message::Enable),
                ),
                ui.row(
                    "Incoming",
                    "Unless a rule says otherwise",
                    ui.choose(&POLICIES, &firewall.incoming, Message::Incoming),
                ),
                ui.row(
                    "Outgoing",
                    "Unless a rule says otherwise",
                    ui.choose(&POLICIES, &firewall.outgoing, Message::Outgoing),
                ),
            ],
        );
        let valid = ufw::valid_port(self.port.trim());
        let protocol = ui.segments(
            PROTOCOLS.iter().enumerate().map(|(index, name)| match *name {
                "any" => (index, "Any"),
                "tcp" => (index, "TCP"),
                _ => (index, "UDP"),
            }),
            &self.protocol,
            Message::Protocol,
            false,
        );
        let action = ui.segments(
            [(0, "Allow"), (1, "Block")],
            &self.action,
            Message::Action,
            false,
        );
        let form = column![
            row![
                ui.field("Port, such as 22 or 8000:8010", &self.port, Message::Port)
                    .on_submit_maybe(valid.then_some(Message::Add)),
                ui.button("Add", Kind::Primary, valid.then_some(Message::Add)),
            ]
            .spacing(8)
            .align_y(Center),
            ui.actions(vec![action, protocol]),
        ]
        .spacing(10);
        let mut rules: Vec<_> = firewall
            .rules
            .iter()
            .map(|rule| {
                let verdict = if rule.action == "allow" { "Allowed" } else { "Blocked" };
                let direction = if rule.incoming { "in" } else { "out" };
                let hint = if rule.source == "any" {
                    format!("{verdict}, {direction}")
                } else {
                    format!("{verdict}, {direction}, from {}", rule.source)
                };
                let delete = Message::Delete(rule.clone());
                ui.row(
                    rule.title(),
                    hint,
                    ui.icon_button(Icon::Close, Tone::Mute, Some(delete)),
                )
            })
            .collect();
        rules.push(ui.cell(form));
        ui.panel(
            "Firewall",
            "",
            None,
            vec![ui.columns(policy, ui.group("Rules", rules))],
        )
    }

    fn summary(&self) -> String {
        match &self.firewall {
            None => String::new(),
            Some(firewall) if !firewall.installed => "Not installed".to_owned(),
            Some(firewall) if firewall.enabled => "On".to_owned(),
            Some(_) => "Off".to_owned(),
        }
    }
}
