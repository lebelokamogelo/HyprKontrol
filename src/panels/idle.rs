use iced::Task;

use super::{Effect, Panel, power};
use crate::icon::Icon;
use crate::sys::idle::{self, Stage};
use crate::sys::services::{self, Scope};
use crate::sys::{Cmd, config_dir, packages};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

const SERVICE: &str = "hypridle.service";

#[derive(Default)]
pub struct State {
    loaded: bool,
    text: Option<String>,
    running: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Option<String>, bool),
    Running(bool),
    Timeout(Stage, Option<u32>),
    LockNow,
    EditLock,
    Install,
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(services::is_active(Scope::User, SERVICE), |running| {
            Message::Loaded(idle::load(), running)
        })
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(text, running) => {
                self.loaded = true;
                self.text = text;
                self.running = running;
                Effect::none()
            }
            Message::Running(on) => {
                Effect::change(services::set_active(Scope::User, SERVICE, on))
            }
            Message::Timeout(stage, seconds) => match &self.text {
                Some(text) => Effect::change(idle::set(text, stage, seconds)),
                None => Effect::none(),
            },
            Message::LockNow => Effect::run(Cmd::new("loginctl").arg("lock-session")),
            Message::EditLock => {
                let path = config_dir().join("hypr/hyprlock.conf").display().to_string();
                Effect::run(Cmd::new("xdg-open").arg(path).detached())
            }
            Message::Install => Effect::checked(packages::install(&["hypridle", "hyprlock"])),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        const TITLE: &str = "Lock and idle";
        if !self.loaded {
            return ui.panel(TITLE, "", None, vec![ui.loading()]);
        }
        let lock = ui.button("Lock now", Kind::Secondary, Some(Message::LockNow));
        let Some(text) = &self.text else {
            let empty = ui.empty(
                Icon::Lock,
                "hypridle is not set up",
                "It dims, locks and suspends when you step away.",
                Some(ui.button("Install", Kind::Primary, Some(Message::Install))),
            );
            return ui.panel(TITLE, "", Some(lock), vec![empty]);
        };
        let service = ui.group(
            "",
            vec![ui.toggle(
                "Act when you step away",
                "Turn off to keep the computer awake",
                self.running,
                Some(Message::Running),
            )],
        );
        let stages = ui.group("While away", power::idle_rows(ui, text, Message::Timeout));
        let screen = ui.group(
            "Lock screen",
            vec![ui.link(
                Icon::Image,
                Tone::Mute,
                "Edit the lock screen",
                "Opens hyprlock.conf",
                Message::EditLock,
            )],
        );
        ui.panel(
            TITLE,
            self.summary(),
            Some(lock),
            vec![service, ui.columns(stages, screen)],
        )
    }

    fn summary(&self) -> String {
        match &self.text {
            None if self.loaded => "Not set up".to_owned(),
            None => String::new(),
            Some(_) if !self.running => "Keeping awake".to_owned(),
            Some(text) => match idle::timeout(text, Stage::Lock) {
                Some(seconds) => {
                    let delay = power::Delay(Some(seconds)).to_string();
                    format!("Locks {}", delay.to_lowercase())
                }
                None => "Does not lock".to_owned(),
            },
        }
    }
}
