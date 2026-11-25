//! Binding between controls and Hyprland options.
//!
//! Several panels are little more than a list of options. This keeps the parts
//! they share in one place: reading values, previewing a slider on the desktop
//! while it moves, and turning the result into a single change with Undo.

use std::collections::HashMap;

use iced::Task;

use super::Effect;
use crate::sys::hypr::{self, Store, Value};
use crate::ui::{Element, Throttle, Ui};

/// Current values of a set of options, and the drag in progress.
#[derive(Debug, Default)]
pub struct Options {
    values: HashMap<String, Value>,
    before: Option<(String, Value)>,
    throttle: Throttle<(String, Value)>,
    loaded: bool,
}

#[derive(Debug, Clone)]
pub enum OptionMessage {
    Loaded(HashMap<String, Value>),
    /// Show on the desktop without saving.
    Preview(&'static str, Value),
    /// Save what is being previewed.
    Commit,
    Set(&'static str, Value),
    Sent,
}

impl Options {
    pub fn load(keys: &'static [&'static str]) -> Task<OptionMessage> {
        Task::perform(hypr::options(keys), OptionMessage::Loaded)
    }

    pub fn ready(&self) -> bool {
        self.loaded
    }

    /// Whether the running Hyprland knows this option. Options come and go
    /// between releases, and controls for missing ones are left out.
    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn number(&self, key: &str) -> f32 {
        self.get(key).map_or(0.0, Value::number)
    }

    pub fn bool(&self, key: &str) -> bool {
        self.get(key).is_some_and(Value::bool)
    }

    pub fn text(&self, key: &str) -> &str {
        self.get(key).map_or("", Value::text)
    }

    pub fn update(&mut self, message: OptionMessage) -> Effect<OptionMessage> {
        match message {
            OptionMessage::Loaded(values) => {
                // A reload in the middle of a drag would make the handle jump.
                if self.before.is_none() {
                    self.values = values;
                    self.loaded = true;
                }
                Effect::none()
            }
            OptionMessage::Preview(key, value) => {
                let previous = self.values.insert(key.to_owned(), value.clone());
                if self.before.as_ref().is_none_or(|(dragged, _)| dragged != key) {
                    self.before = previous.map(|previous| (key.to_owned(), previous));
                }
                self.send((key.to_owned(), value))
            }
            OptionMessage::Sent => match self.throttle.done() {
                Some((key, value)) => Effect::task(preview(key, value)),
                None => Effect::none(),
            },
            OptionMessage::Commit => {
                self.throttle.clear();
                let Some((key, before)) = self.before.take() else { return Effect::none() };
                match self.values.get(&key) {
                    Some(after) if *after != before => {
                        Effect::change(Store::load().set(&key, before, after.clone()))
                    }
                    _ => Effect::none(),
                }
            }
            OptionMessage::Set(key, value) => {
                let before = self.values.insert(key.to_owned(), value.clone());
                match before {
                    Some(before) => Effect::change(Store::load().set(key, before, value)),
                    None => Effect::none(),
                }
            }
        }
    }

    fn send(&mut self, request: (String, Value)) -> Effect<OptionMessage> {
        match self.throttle.push(request) {
            Some((key, value)) => Effect::task(preview(key, value)),
            None => Effect::none(),
        }
    }

    /// A switch bound to an option.
    pub fn toggle<'a, M: Clone + 'a>(
        &self,
        ui: Ui<'a>,
        key: &'static str,
        label: &'static str,
        hint: &'static str,
        wrap: fn(OptionMessage) -> M,
    ) -> Option<Element<'a, M>> {
        // Some options are integers that act as switches. They keep their type.
        let integer = matches!(self.get(key)?, Value::Int(_));
        Some(ui.toggle(
            label,
            hint,
            self.bool(key),
            Some(move |on: bool| {
                let value = if integer { Value::Int(on.into()) } else { Value::Bool(on) };
                wrap(OptionMessage::Set(key, value))
            }),
        ))
    }

    /// A slider bound to an option, previewed while it moves.
    #[allow(clippy::too_many_arguments)]
    pub fn slide<'a, M: Clone + 'a>(
        &self,
        ui: Ui<'a>,
        key: &'static str,
        label: &'static str,
        range: std::ops::RangeInclusive<f32>,
        step: f32,
        show: fn(f32) -> String,
        wrap: fn(OptionMessage) -> M,
    ) -> Option<Element<'a, M>> {
        let integer = matches!(self.get(key)?, Value::Int(_));
        let value = self.number(key);
        Some(ui.slide(
            label,
            range,
            step,
            value,
            show(value),
            move |value| {
                let value =
                    if integer { Value::Int(value.round() as i64) } else { Value::float(value) };
                wrap(OptionMessage::Preview(key, value))
            },
            wrap(OptionMessage::Commit),
        ))
    }
}

fn preview(key: String, value: Value) -> Task<OptionMessage> {
    Task::perform(hypr::preview(key, value), |_| OptionMessage::Sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::Op;
    use crate::sys::hypr::Edit;

    const KEY: &str = "decoration.rounding";

    fn loaded() -> Options {
        let mut options = Options::default();
        let values = HashMap::from([(KEY.to_owned(), Value::Int(10))]);
        let _ = options.update(OptionMessage::Loaded(values));
        options
    }

    #[test]
    fn a_drag_becomes_one_change_that_undoes_to_its_start() {
        let mut options = loaded();
        for value in [11, 14, 18] {
            let effect = options.update(OptionMessage::Preview(KEY, Value::Int(value)));
            assert!(effect.change.is_none(), "previews are not saved");
        }
        let change = options.update(OptionMessage::Commit).change.unwrap();
        let edit = |op: Op| match op {
            Op::Hypr(mut edits) => edits.remove(0),
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(edit(change.apply), Edit::Set(KEY.into(), Value::Int(18)));
        assert!(matches!(
            edit(change.undo.unwrap()),
            Edit::Reset(_, Value::Int(10)) | Edit::Set(_, Value::Int(10))
        ));
    }

    #[test]
    fn a_drag_back_to_the_start_changes_nothing() {
        let mut options = loaded();
        let _ = options.update(OptionMessage::Preview(KEY, Value::Int(14)));
        let _ = options.update(OptionMessage::Preview(KEY, Value::Int(10)));
        assert!(options.update(OptionMessage::Commit).change.is_none());
    }

    #[test]
    fn a_fractional_drag_back_to_the_start_changes_nothing() {
        const SPEED: &str = "input.sensitivity";
        let mut options = Options::default();
        let values = HashMap::from([(SPEED.to_owned(), Value::Float(0.0))]);
        let _ = options.update(OptionMessage::Loaded(values));
        let _ = options.update(OptionMessage::Preview(SPEED, Value::float(0.15)));
        let _ = options.update(OptionMessage::Preview(SPEED, Value::float(1.490_116_1e-8)));
        assert!(options.update(OptionMessage::Commit).change.is_none());
    }

    #[test]
    fn a_reload_does_not_interrupt_a_drag() {
        let mut options = loaded();
        let _ = options.update(OptionMessage::Preview(KEY, Value::Int(14)));
        let stale = HashMap::from([(KEY.to_owned(), Value::Int(10))]);
        let _ = options.update(OptionMessage::Loaded(stale));
        assert_eq!(options.number(KEY), 14.0);
    }
}
