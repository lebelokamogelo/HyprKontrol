use iced::Task;
use iced::widget::row;

use super::options::{OptionMessage, Options};
use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::hypr::Value;
use crate::sys::region::{self, Layout, Layouts};
use crate::theme::Tone;
use crate::ui::{Element, Ui};

const KEYS: &[&str] = &[
    "input.kb_layout",
    "input.kb_variant",
    "input.kb_options",
    "input.repeat_rate",
    "input.repeat_delay",
    "input.numlock_by_default",
];

/// Frequently wanted XKB options. The full list has several hundred entries.
const OPTIONS: [(&str, &str); 7] = [
    ("ctrl:nocaps", "Caps Lock acts as Ctrl"),
    ("caps:escape", "Caps Lock acts as Escape"),
    ("caps:swapescape", "Swap Caps Lock and Escape"),
    ("altwin:swap_alt_win", "Swap Alt and Super"),
    ("compose:ralt", "Right Alt is the Compose key"),
    ("grp:alt_shift_toggle", "Alt+Shift switches layout"),
    ("grp:win_space_toggle", "Super+Space switches layout"),
];

#[derive(Default)]
pub struct State {
    options: Options,
    known: Layouts,
    query: String,
    adding: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Option(OptionMessage),
    Known(Layouts),
    Add(String),
    Remove(usize),
    Variant(usize, String),
    XkbOption(&'static str, bool),
    Query(String),
    Adding,
}

/// Layouts in use, each with its variant. XKB keeps them as two parallel lists.
fn pairs(layouts: &str, variants: &str) -> Vec<(String, String)> {
    let mut variants = variants.split(',').map(str::trim);
    layouts
        .split(',')
        .map(str::trim)
        .filter(|layout| !layout.is_empty())
        .map(|layout| (layout.to_owned(), variants.next().unwrap_or_default().to_owned()))
        .collect()
}

fn join(pairs: &[(String, String)]) -> (String, String) {
    let layouts: Vec<_> = pairs.iter().map(|(layout, _)| layout.as_str()).collect();
    let variants: Vec<_> = pairs.iter().map(|(_, variant)| variant.as_str()).collect();
    // A list of empty variants is written as an empty string.
    let empty = variants.iter().all(|variant| variant.is_empty());
    (layouts.join(","), if empty { String::new() } else { variants.join(",") })
}

impl State {
    fn pairs(&self) -> Vec<(String, String)> {
        pairs(self.options.text("input.kb_layout"), self.options.text("input.kb_variant"))
    }

    fn name(&self, code: &str) -> String {
        let known = self.known.layouts.iter().find(|layout| layout.code == code);
        known.map_or_else(|| code.to_owned(), |layout| layout.name.clone())
    }

    /// Layouts and variants only make sense together, so both are written as one
    /// change with one Undo.
    fn set(&mut self, edit: impl FnOnce(&mut Vec<(String, String)>)) -> Effect<Message> {
        use crate::sys::hypr::{Edit, Store};
        use crate::sys::{Change, Op};

        let before = self.pairs();
        let mut after = before.clone();
        edit(&mut after);
        if after.is_empty() || after == before {
            return Effect::none();
        }
        let store = Store::load();
        let edits = |pairs: &[(String, String)], reset: bool| {
            let (layouts, variants) = join(pairs);
            let one = |key: &str, text: String| {
                let value = Value::Text(text);
                if reset && !store.options.contains_key(key) {
                    Edit::Reset(key.to_owned(), value)
                } else {
                    Edit::Set(key.to_owned(), value)
                }
            };
            Op::Hypr(vec![one("input.kb_layout", layouts), one("input.kb_variant", variants)])
        };
        Effect::change(Change::new(edits(&after, false)).undo(edits(&before, true)))
    }

    fn layouts<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let pairs = self.pairs();
        let single = pairs.len() == 1;
        let mut rows: Vec<_> = pairs
            .iter()
            .enumerate()
            .map(|(index, (layout, variant))| {
                let standard = Layout { code: String::new(), name: "Standard".to_owned() };
                let mut variants = vec![standard.clone()];
                variants.extend(
                    self.known
                        .variants
                        .iter()
                        .filter(|(known, _)| known == layout)
                        .map(|(_, variant)| variant.clone()),
                );
                let selected = variants.iter().find(|v| v.code == *variant).cloned();
                let controls = row![
                    ui.pick(variants, selected, move |v| Message::Variant(index, v.code)),
                    ui.icon_button(
                        Icon::Close,
                        Tone::Mute,
                        (!single).then_some(Message::Remove(index)),
                    ),
                ]
                .spacing(8)
                .align_y(iced::Center);
                ui.row(self.name(layout), layout.clone(), controls)
            })
            .collect();
        rows.push(ui.link(
            Icon::Plus,
            Tone::Accent,
            "Add a layout",
            "",
            Message::Adding,
        ));
        if self.adding {
            let query = self.query.to_lowercase();
            let matches = self
                .known
                .layouts
                .iter()
                .filter(move |layout| {
                    query.is_empty()
                        || layout.name.to_lowercase().contains(&query)
                        || layout.code == query
                })
                .map(|layout| (layout.name.clone(), false, Message::Add(layout.code.clone())));
            rows.push(ui.finder(&self.query, Message::Query, matches));
        }
        ui.group("Layouts", rows)
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        let options = Options::load(KEYS).map(Message::Option);
        if self.known.layouts.is_empty() {
            let known = Task::future(async { Message::Known(region::layouts()) });
            return Task::batch([options, known]);
        }
        options
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Option(message) => self.options.update(message).map(Message::Option),
            Message::Known(known) => {
                self.known = known;
                Effect::none()
            }
            Message::Adding => {
                self.adding = !self.adding;
                self.query.clear();
                Effect::none()
            }
            Message::Query(query) => {
                self.query = query;
                Effect::none()
            }
            Message::Add(code) => {
                self.adding = false;
                self.set(|pairs| {
                    if !pairs.iter().any(|(layout, _)| *layout == code) {
                        pairs.push((code, String::new()));
                    }
                })
            }
            Message::Remove(index) => self.set(|pairs| drop(pairs.remove(index))),
            Message::Variant(index, variant) => self.set(|pairs| pairs[index].1 = variant),
            Message::XkbOption(option, on) => {
                let current = self.options.text("input.kb_options");
                let mut list: Vec<&str> =
                    current.split(',').map(str::trim).filter(|o| !o.is_empty()).collect();
                list.retain(|known| *known != option);
                if on {
                    list.push(option);
                }
                let value = Value::Text(list.join(","));
                let set = OptionMessage::Set("input.kb_options", value);
                self.options.update(set).map(Message::Option)
            }
        }
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        if !self.options.ready() {
            return ui.panel("Keyboard", "", None, vec![ui.loading()]);
        }
        let o = &self.options;
        let mut typing = Vec::new();
        typing.extend(o.slide(
            ui,
            "input.repeat_delay",
            "Repeat after",
            150.0..=1000.0,
            25.0,
            |value| format!("{value:.0} ms"),
            Message::Option,
        ));
        typing.extend(o.slide(
            ui,
            "input.repeat_rate",
            "Repeat speed",
            5.0..=80.0,
            1.0,
            |value| format!("{value:.0}/s"),
            Message::Option,
        ));
        typing.extend(o.toggle(
            ui,
            "input.numlock_by_default",
            "Num Lock on at login",
            "",
            Message::Option,
        ));
        let active: Vec<&str> = o.text("input.kb_options").split(',').map(str::trim).collect();
        let switches = OPTIONS
            .into_iter()
            .map(|(option, label)| {
                ui.toggle(
                    label,
                    "",
                    active.contains(&option),
                    Some(move |on| Message::XkbOption(option, on)),
                )
            })
            .collect();
        ui.panel(
            "Keyboard",
            self.summary(),
            None,
            vec![ui.columns(
                ui.stack(vec![self.layouts(ui), ui.group("Typing", typing)]),
                ui.group("Special keys", switches),
            )],
        )
    }

    fn summary(&self) -> String {
        match self.pairs().as_slice() {
            [] => String::new(),
            [(only, _)] => self.name(only),
            several => format!("{} layouts", several.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_and_variants_stay_aligned() {
        let parsed = pairs("us,za,de", "dvorak,");
        assert_eq!(parsed[0], ("us".into(), "dvorak".into()));
        assert_eq!(parsed[2], ("de".into(), String::new()));
        assert_eq!(join(&parsed), ("us,za,de".into(), "dvorak,,".into()));
        assert_eq!(join(&pairs("us", "")), ("us".into(), String::new()));
    }
}
