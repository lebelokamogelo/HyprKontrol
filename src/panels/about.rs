use iced::widget::row;
use iced::{Center, Task};

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::probe::{self, Machine};
use crate::sys::region;
use crate::sys::{self, Cmd};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    machine: Option<Machine>,
    hostname: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Machine>),
    Hostname(String),
    Rename,
    Copy,
    Open(&'static str),
}

const LINKS: [(&str, &str); 3] = [
    ("CachyOS wiki", "https://wiki.cachyos.org"),
    ("Hyprland wiki", "https://wiki.hypr.land"),
    ("CachyOS forum", "https://discuss.cachyos.org"),
];

/// Host names follow RFC 1123.
fn valid_hostname(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

impl State {
    /// Label and value of every fact that is known.
    fn facts(machine: &Machine) -> Vec<(&'static str, String)> {
        let threads = format!("{} ({} threads)", machine.cpu, machine.threads);
        [
            ("System", machine.os.clone()),
            ("Kernel", machine.kernel.clone()),
            ("Desktop", machine.desktop.clone()),
            ("Model", machine.model.clone()),
            ("Firmware", machine.firmware.clone()),
            ("Processor", threads),
            ("Graphics", machine.gpus.join(", ")),
            ("Memory", sys::bytes(machine.memory)),
            ("Uptime", machine.uptime.clone()),
        ]
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .collect()
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(probe::machine(), |machine| Message::Loaded(Box::new(machine)))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(machine) => {
                self.hostname.clone_from(&machine.hostname);
                self.machine = Some(*machine);
                Effect::none()
            }
            Message::Hostname(hostname) => {
                self.hostname = hostname;
                Effect::none()
            }
            Message::Rename => match &self.machine {
                Some(machine) => {
                    Effect::change(region::set_hostname(&machine.hostname, self.hostname.trim()))
                }
                None => Effect::none(),
            },
            Message::Copy => match &self.machine {
                Some(machine) => {
                    let text: Vec<_> = Self::facts(machine)
                        .into_iter()
                        .map(|(label, value)| format!("{label}: {value}"))
                        .collect();
                    Effect::request(super::Request::Done("Copied".to_owned()))
                        .and(iced::clipboard::write(text.join("\n")))
                }
                None => Effect::none(),
            },
            Message::Open(url) => Effect::run(Cmd::new("xdg-open").arg(url).detached()),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(machine) = &self.machine else {
            return ui.panel("About", "", None, vec![ui.loading()]);
        };
        let copy = ui.button("Copy details", Kind::Secondary, Some(Message::Copy));
        let name = self.hostname.trim();
        let renamed = name != machine.hostname && valid_hostname(name);
        let rename = row![
            ui.field("Name", &self.hostname, Message::Hostname)
                .on_submit_maybe(renamed.then_some(Message::Rename)),
            ui.button("Rename", Kind::Secondary, renamed.then_some(Message::Rename)),
        ]
        .spacing(8)
        .align_y(Center);
        let identity = ui.group("Name on the network", vec![ui.cell(rename)]);
        let facts = Self::facts(machine)
            .into_iter()
            .map(|(label, value)| ui.value(label, value))
            .collect();
        let links = LINKS
            .into_iter()
            .map(|(label, url)| {
                let host = url.trim_start_matches("https://");
                ui.link(Icon::Globe, Tone::Mute, label, host, Message::Open(url))
            })
            .collect();
        let program = ui.group(
            "This program",
            vec![ui.value("HyprKontrol", env!("CARGO_PKG_VERSION"))],
        );
        ui.panel(
            "About",
            machine.model.as_str(),
            Some(copy),
            vec![ui.split(
                3,
                ui.group("This computer", facts),
                2,
                ui.stack(vec![identity, ui.group("Help", links), program]),
            )],
        )
    }

    fn summary(&self) -> String {
        self.machine.as_ref().map(|machine| machine.os.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names_follow_the_standard() {
        assert!(valid_hostname("cachy-laptop"));
        assert!(!valid_hostname("-x") && !valid_hostname("a b") && !valid_hostname(""));
    }
}
