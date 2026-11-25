use iced::Task;

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::packages::{self, Kernel};
use crate::sys::{Cmd, installed};
use crate::theme::Tone;
use crate::ui::{Element, Kind, Ui};

#[derive(Default)]
pub struct State {
    kernels: Option<Vec<Kernel>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Vec<Kernel>),
    Install(String),
    Remove(String),
    Manager,
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(packages::kernels(), Message::Loaded)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(kernels) => {
                self.kernels = Some(kernels);
                Effect::none()
            }
            Message::Install(package) => Effect::session(packages::install_kernel(&package)),
            Message::Remove(package) => Effect::session(packages::remove_kernel(&package)),
            Message::Manager => Effect::run(Cmd::new("cachyos-kernel-manager").detached()),
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(kernels) = &self.kernels else {
            return ui.panel("Kernels", "", None, vec![ui.loading()]);
        };
        let manager = installed("cachyos-kernel-manager")
            .then(|| ui.button("Build a custom kernel", Kind::Secondary, Some(Message::Manager)));
        let (present, available): (Vec<_>, Vec<_>) =
            kernels.iter().partition(|kernel| kernel.installed.is_some());
        let only_one = present.len() < 2;

        let present = present
            .into_iter()
            .map(|kernel| {
                let version = kernel.installed.as_deref().unwrap_or_default();
                let hint = match (kernel.running, version == kernel.available) {
                    (true, _) => format!("{version}, running now"),
                    (false, true) => version.to_owned(),
                    (false, false) => format!("{version}, {} is available", kernel.available),
                };
                // Removing the running or the last kernel leaves nothing to boot.
                let removable = !kernel.running && !only_one;
                let remove = Message::Remove(kernel.package.clone());
                let tone = if kernel.running { Tone::Accent } else { Tone::Mute };
                ui.icon_row(
                    Icon::Cachy,
                    tone,
                    kernel.label(),
                    hint,
                    ui.button("Remove", Kind::Danger, removable.then_some(remove)),
                )
            })
            .collect();
        let available = available
            .into_iter()
            .map(|kernel| {
                let hint = match kernel.blurb() {
                    "" => kernel.available.clone(),
                    blurb => format!("{blurb}, {}", kernel.available),
                };
                let install = Message::Install(kernel.package.clone());
                ui.row(
                    kernel.label(),
                    hint,
                    ui.button("Install", Kind::Secondary, Some(install)),
                )
            })
            .collect();
        ui.panel(
            "Kernels",
            "Restart to switch to another one",
            manager,
            vec![ui.columns(
                ui.group("Installed", present),
                ui.group("Available from CachyOS", available),
            )],
        )
    }

    fn summary(&self) -> String {
        let Some(kernels) = &self.kernels else { return String::new() };
        let count = kernels.iter().filter(|k| k.installed.is_some()).count();
        match kernels.iter().find(|kernel| kernel.running) {
            Some(kernel) => {
                let version = kernel.installed.as_deref().unwrap_or_default();
                format!("{version}, {count} installed")
            }
            None => format!("{count} installed"),
        }
    }
}
