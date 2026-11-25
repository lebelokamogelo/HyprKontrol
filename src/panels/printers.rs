use iced::Task;

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::services::{self, Scope};
use crate::sys::{Change, Cmd, installed, packages, query};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

const SOCKET: &str = "cups.socket";
/// CUPS manages itself through a web page, which every version has.
const ADMIN: &str = "http://localhost:631/admin";

#[derive(Debug, Clone, Default)]
pub struct Data {
    installed: bool,
    running: bool,
    printers: Vec<Printer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Printer {
    name: String,
    state: String,
    default: bool,
}

#[derive(Default)]
pub struct State {
    data: Option<Data>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Data),
    Install,
    Service(bool),
    Default(String),
    Manage,
}

async fn load() -> Data {
    if !installed("lpstat") {
        return Data::default();
    }
    let (running, text) = tokio::join!(
        services::is_active(Scope::System, SOCKET),
        query("lpstat", ["-p", "-d"])
    );
    Data { installed: true, running, printers: parse(&text.unwrap_or_default()) }
}

/// Parses `printer HP_Laser is idle.  enabled since …` and the default line.
fn parse(text: &str) -> Vec<Printer> {
    let default = text
        .lines()
        .find_map(|line| line.strip_prefix("system default destination: "))
        .unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let (name, state) = line.strip_prefix("printer ")?.split_once(' ')?;
            let state = state.trim_start_matches("is ").split('.').next()?;
            Some(Printer {
                name: name.to_owned(),
                state: state.trim().to_owned(),
                default: name == default,
            })
        })
        .collect()
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(load(), Message::Loaded)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(data) => {
                self.data = Some(data);
                Effect::none()
            }
            Message::Install => Effect::checked(packages::install(&["cups", "cups-pdf"])),
            Message::Service(on) => Effect::change(services::set_now(Scope::System, SOCKET, on)),
            Message::Default(name) => {
                let set = |name: &str| Cmd::new("lpoptions").args(["-d", name]);
                let before = self.data.as_ref().and_then(|d| d.printers.iter().find(|p| p.default));
                let change = Change::new(set(&name));
                Effect::change(match before {
                    Some(printer) => change.undo(set(&printer.name)),
                    None => change,
                })
            }
            Message::Manage => Effect::run(Cmd::new("xdg-open").arg(ADMIN).detached()),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let body = match &self.data {
            None => vec![ui.loading()],
            Some(data) if !data.installed => vec![ui.empty(
                Icon::Printer,
                "Printing is not set up",
                "CUPS is the print service used on Linux. Installing it opens a terminal.",
                Some(ui.button("Install CUPS", Kind::Primary, Some(Message::Install))),
            )],
            Some(data) => {
                let mut body = vec![ui.group(
                    "",
                    vec![ui.toggle(
                        "Print service",
                        "Starts when something is printed",
                        data.running,
                        Some(Message::Service),
                    )],
                )];
                if data.running {
                    let mut rows: Vec<_> = data
                        .printers
                        .iter()
                        .map(|printer| {
                            ui.option(
                                Icon::Printer,
                                printer.name.as_str(),
                                printer.state.as_str(),
                                printer.default,
                                Message::Default(printer.name.clone()),
                            )
                        })
                        .collect();
                    rows.push(ui.link(
                        Icon::Plus,
                        Tone::Accent,
                        "Add or manage printers",
                        "Opens the CUPS page in your browser",
                        Message::Manage,
                    ));
                    body.push(ui.group("Printers", rows));
                }
                body
            }
        };
        ui.panel("Printers", "", None, body)
    }

    fn summary(&self) -> String {
        match &self.data {
            None => String::new(),
            Some(data) if !data.installed => "Not set up".to_owned(),
            Some(data) if !data.running => "Print service is off".to_owned(),
            Some(data) => match data.printers.iter().find(|p| p.default) {
                Some(printer) => printer.name.clone(),
                None => format!("{} printers", data.printers.len()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lpstat() {
        let printers = parse(
            "printer HP is idle.  enabled since Mon\nprinter PDF disabled since Tue -\n\
             system default destination: HP\n",
        );
        assert_eq!(printers.len(), 2);
        assert!(printers[0].default);
        assert_eq!(printers[0].state, "idle");
    }
}
