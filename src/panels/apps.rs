use iced::Task;

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::desktop::{self, App, ROLES, Startup};
use crate::theme::Tone;
use crate::ui::{Element, Ui};

#[derive(Debug, Clone, Default)]
pub struct Data {
    apps: Vec<App>,
    /// Desktop entry id per role, in the order of [`ROLES`].
    defaults: Vec<String>,
    startup: Vec<Startup>,
}

#[derive(Default)]
pub struct State {
    data: Option<Data>,
    adding: bool,
    query: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Data),
    Default(usize, App),
    Startup(String, bool),
    Adding,
    Query(String),
    Add(App),
}

async fn load() -> Data {
    let mut defaults = Vec::with_capacity(ROLES.len());
    for role in ROLES {
        defaults.push(desktop::default_app(role.mime[0]).await);
    }
    Data { apps: desktop::apps(), defaults, startup: desktop::startup() }
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
            Message::Default(index, app) => {
                let before = self.data.as_ref().map_or("", |data| &data.defaults[index]);
                Effect::change(desktop::set_default_app(&ROLES[index], before, &app.id))
            }
            Message::Startup(file, on) => Effect::change(desktop::set_startup(&file, on)),
            Message::Adding => {
                self.adding = !self.adding;
                self.query.clear();
                Effect::none()
            }
            Message::Query(query) => {
                self.query = query;
                Effect::none()
            }
            Message::Add(app) => {
                self.adding = false;
                Effect::attempt(desktop::add_startup(&app))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        const TITLE: &str = "Apps and startup";
        let Some(data) = &self.data else {
            return ui.panel(TITLE, "", None, vec![ui.loading()]);
        };
        let defaults = ROLES
            .iter()
            .enumerate()
            .map(|(index, role)| {
                let current = &data.defaults[index];
                let options: Vec<App> = data
                    .apps
                    .iter()
                    .filter(|app| app.id == *current || app.mime.iter().any(|m| m == role.mime[0]))
                    .cloned()
                    .collect();
                if options.is_empty() {
                    return ui.value(role.label, "No app installed");
                }
                let selected = options.iter().find(|app| app.id == *current).cloned();
                ui.row(
                    role.label,
                    "",
                    ui.pick(options, selected, move |app| Message::Default(index, app)),
                )
            })
            .collect();

        let mut startup: Vec<_> = data
            .startup
            .iter()
            .map(|entry| {
                let file = entry.file.clone();
                let hint = match (entry.comment.is_empty(), entry.system) {
                    (false, _) => entry.comment.as_str(),
                    (true, true) => "Part of the system",
                    (true, false) => "",
                };
                ui.toggle(
                    entry.name.as_str(),
                    hint,
                    entry.enabled,
                    Some(move |on| Message::Startup(file.clone(), on)),
                )
            })
            .collect();
        startup.push(ui.link(Icon::Plus, Tone::Accent, "Add an app", "", Message::Adding));
        if self.adding {
            let query = self.query.to_lowercase();
            let matches = data
                .apps
                .iter()
                .filter(move |app| query.is_empty() || app.name.to_lowercase().contains(&query))
                .map(|app| (app.name.clone(), false, Message::Add(app.clone())));
            startup.push(ui.finder(&self.query, Message::Query, matches));
        }
        ui.panel(
            TITLE,
            "",
            None,
            vec![ui.columns(
                ui.group("Open with", defaults),
                ui.group("Start when you log in", startup),
            )],
        )
    }

    fn summary(&self) -> String {
        match &self.data {
            Some(data) => {
                let count = data.startup.iter().filter(|entry| entry.enabled).count();
                format!("{count} start at login")
            }
            None => String::new(),
        }
    }
}
