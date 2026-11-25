use iced::Task;

use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::region::{self, Locale};
use crate::theme::Tone;
use crate::ui::{Element, Ui};

#[derive(Default)]
pub struct State {
    locale: Option<Locale>,
    generatable: Vec<String>,
    adding: bool,
    query: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Locale, Vec<String>),
    Language(String),
    Formats(String),
    Adding,
    Query(String),
    Generate(String),
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Task::perform(region::locale(), |locale| Message::Loaded(locale, region::generatable()))
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Loaded(locale, generatable) => {
                self.locale = Some(locale);
                self.generatable = generatable;
                Effect::none()
            }
            Message::Language(language) => match &self.locale {
                Some(l) => Effect::change(region::set_locale(
                    (&l.language, &l.formats),
                    (&language, &l.formats),
                )),
                None => Effect::none(),
            },
            Message::Formats(formats) => match &self.locale {
                Some(l) => Effect::change(region::set_locale(
                    (&l.language, &l.formats),
                    (&l.language, &formats),
                )),
                None => Effect::none(),
            },
            Message::Adding => {
                self.adding = !self.adding;
                self.query.clear();
                Effect::none()
            }
            Message::Query(query) => {
                self.query = query;
                Effect::none()
            }
            Message::Generate(name) => {
                self.adding = false;
                Effect::checked(region::generate_locale(&name))
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let Some(locale) = &self.locale else {
            return ui.panel("Language", "", None, vec![ui.loading()]);
        };
        let pick = |selected: &str, message: fn(String) -> Message| {
            ui.pick(locale.available.clone(), Some(selected.to_owned()), message)
        };
        let chosen = ui.group(
            "",
            vec![
                ui.row(
                    "Language",
                    "Menus and messages",
                    pick(&locale.language, Message::Language),
                ),
                ui.row(
                    "Formats",
                    "Dates, numbers, currency and paper size",
                    pick(&locale.formats, Message::Formats),
                ),
            ],
        );
        let mut more = vec![ui.link(
            Icon::Plus,
            Tone::Accent,
            "Add a language",
            "Generates it with locale-gen",
            Message::Adding,
        )];
        if self.adding {
            let query = self.query.to_lowercase();
            let matches = self
                .generatable
                .iter()
                .filter(move |name| query.is_empty() || name.to_lowercase().contains(&query))
                .map(|name| (name.clone(), false, Message::Generate(name.clone())));
            more.push(ui.finder(&self.query, Message::Query, matches));
        }
        ui.panel(
            "Language",
            "Takes effect the next time you log in",
            None,
            vec![ui.columns(chosen, ui.group("", more))],
        )
    }

    fn summary(&self) -> String {
        self.locale.as_ref().map(|locale| locale.language.clone()).unwrap_or_default()
    }
}
