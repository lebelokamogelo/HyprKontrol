use iced::widget::column;
use iced::{Color, Task};

use super::options::{OptionMessage, Options};
use super::{Effect, Panel};
use crate::icon::Icon;
use crate::sys::hypr::{self, Value};
use crate::theme::color::Hex;
use crate::ui::color::{ColorEdit, ColorMessage};
use crate::ui::{Element, Ui, chart};

const KEYS: &[&str] = &[
    "general.gaps_in",
    "general.gaps_out",
    "general.border_size",
    "general.layout",
    "general.resize_on_border",
    "general.col.active_border",
    "general.col.inactive_border",
    "decoration.rounding",
    "decoration.active_opacity",
    "decoration.inactive_opacity",
    "decoration.dim_inactive",
    "decoration.dim_strength",
    "decoration.blur.enabled",
    "decoration.blur.size",
    "decoration.blur.passes",
    "decoration.shadow.enabled",
    "decoration.shadow.range",
];

const ACTIVE: &str = "general.col.active_border";
const INACTIVE: &str = "general.col.inactive_border";

const LAYOUTS: [(&str, &str); 4] = [
    ("dwindle", "Dwindle"),
    ("master", "Master"),
    ("scrolling", "Scrolling"),
    ("monocle", "Monocle"),
];

/// A border colour: which option, and which stop of its gradient.
type Stop = (&'static str, usize);

#[derive(Default)]
pub struct State {
    options: Options,
    editing: Option<Stop>,
    color: ColorEdit,
}

#[derive(Debug, Clone)]
pub enum Message {
    Option(OptionMessage),
    Layout(&'static str),
    Edit(Stop),
    Color(ColorMessage),
}

/// Colours of a gradient option, with fallbacks for themes that use one colour.
fn stops(value: Option<&Value>) -> Vec<Color> {
    match value {
        Some(Value::Gradient { colors, .. }) => colors.iter().filter_map(|c| parse(c)).collect(),
        _ => Vec::new(),
    }
}

/// Parses `rgba(82dcccff)`.
fn parse(text: &str) -> Option<Color> {
    let digits = text.strip_prefix("rgba(")?.strip_suffix(')')?;
    digits.parse::<Hex>().ok().map(|Hex(color)| color)
}

fn format(color: Color) -> String {
    let [r, g, b, a] = color.into_rgba8();
    format!("rgba({r:02x}{g:02x}{b:02x}{a:02x})")
}

fn with_stop(value: &Value, index: usize, color: Color) -> Value {
    let Value::Gradient { colors, angle } = value else {
        return Value::Gradient { colors: vec![format(color)], angle: 0 };
    };
    let mut colors = colors.clone();
    if colors.len() <= index {
        let last = colors.last().cloned().unwrap_or_else(|| format(color));
        colors.resize(index + 1, last);
    }
    colors[index] = format(color);
    Value::Gradient { colors, angle: *angle }
}

impl State {
    fn stop(&self, (key, index): Stop) -> Color {
        let stops = stops(self.options.get(key));
        stops.get(index).or(stops.last()).copied().unwrap_or(Color::BLACK)
    }

    fn preview<'a>(&self, ui: Ui<'a>) -> Element<'a, Message> {
        let o = &self.options;
        let tiling = chart::Tiling {
            gaps_in: o.number("general.gaps_in"),
            gaps_out: o.number("general.gaps_out"),
            border: o.number("general.border_size"),
            rounding: o.number("decoration.rounding"),
            active: (self.stop((ACTIVE, 0)), self.stop((ACTIVE, 1))),
            inactive: self.stop((INACTIVE, 0)),
            inactive_opacity: o.number("decoration.inactive_opacity"),
        };
        column![tiling.view(ui.d(330.0)), ui.small("Shown at actual size")].spacing(8).into()
    }

    fn borders<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        let swatch = |stop: Stop| ui.swatch(self.stop(stop), Some(Message::Edit(stop)));
        let mut rows = Vec::new();
        if self.options.has(ACTIVE) {
            rows.push(ui.row(
                "Active window",
                "Start and end of the gradient",
                ui.actions(vec![swatch((ACTIVE, 0)), swatch((ACTIVE, 1))]),
            ));
        }
        if self.options.has(INACTIVE) {
            rows.push(ui.row("Other windows", "", swatch((INACTIVE, 0))));
        }
        if self.editing.is_some() {
            rows.push(ui.cell(self.color.view(ui, true, Message::Color)));
        }
        rows.extend(self.options.toggle(
            ui,
            "general.resize_on_border",
            "Resize by dragging borders",
            "",
            Message::Option,
        ));
        ui.group("Borders", rows)
    }

    fn percent<'a>(
        &self,
        ui: Ui<'a>,
        key: &'static str,
        label: &'static str,
    ) -> Option<Element<'a, Message>> {
        self.options.slide(
            ui,
            key,
            label,
            0.2..=1.0,
            0.05,
            |value| format!("{:.0}%", value * 100.0),
            Message::Option,
        )
    }
}

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Options::load(KEYS).map(Message::Option)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        let message = match message {
            Message::Option(message) => message,
            Message::Layout(name) => {
                OptionMessage::Set("general.layout", Value::Text(name.to_owned()))
            }
            Message::Edit(stop) => {
                self.editing = (self.editing != Some(stop)).then_some(stop);
                self.color = ColorEdit::new(self.stop(stop));
                return Effect::none();
            }
            Message::Color(ColorMessage::Done) => OptionMessage::Commit,
            Message::Color(message) => {
                let (Some((key, index)), Some(color)) = (self.editing, self.color.update(message))
                else {
                    return Effect::none();
                };
                let Some(value) = self.options.get(key) else { return Effect::none() };
                OptionMessage::Preview(key, with_stop(value, index, color))
            }
        };
        self.options.update(message).map(Message::Option)
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        const TITLE: &str = "Windows and gaps";
        if !self.options.ready() {
            return ui.panel(TITLE, "", None, vec![ui.loading()]);
        }
        let o = &self.options;
        let pixels = |key, label, max: f32| {
            o.slide(
                ui,
                key,
                label,
                0.0..=max,
                1.0,
                |value| format!("{value:.0} px"),
                Message::Option,
            )
        };
        let plain = |key, label, max: f32| {
            o.slide(ui, key, label, 1.0..=max, 1.0, |v| format!("{v:.0}"), Message::Option)
        };
        let toggle = |key, label, hint| o.toggle(ui, key, label, hint, Message::Option);

        let gaps = [
            pixels("general.gaps_in", "Between windows", 20.0),
            pixels("general.gaps_out", "Screen edge", 40.0),
            pixels("general.border_size", "Border", 8.0),
            pixels("decoration.rounding", "Corner radius", 24.0),
        ];
        let glass = [
            toggle("decoration.blur.enabled", "Blur behind windows", ""),
            plain("decoration.blur.size", "Blur size", 12.0),
            plain("decoration.blur.passes", "Blur passes", 6.0),
            self.percent(ui, "decoration.active_opacity", "Active opacity"),
            self.percent(ui, "decoration.inactive_opacity", "Inactive opacity"),
            toggle("decoration.dim_inactive", "Dim inactive windows", ""),
            toggle("decoration.shadow.enabled", "Shadows", ""),
            pixels("decoration.shadow.range", "Shadow size", 40.0),
        ];

        let layout = o.text("general.layout");
        let current = LAYOUTS.iter().find(|(name, _)| *name == layout).map_or("", |l| l.0);
        let segments = ui.segments(
            LAYOUTS.into_iter().filter(|(name, _)| *name != "monocle" || current == "monocle"),
            &current,
            Message::Layout,
            true,
        );
        let mut body = Vec::new();
        if !hypr::persistent() {
            body.push(ui.notice(
                Icon::Info,
                "No hyprland.lua was found, so changes here last until you log out.",
            ));
        }
        body.push(ui.split(
            2,
            ui.stack(vec![
                ui.group("Gaps and borders", gaps.into_iter().flatten().collect()),
                ui.group("Glass", glass.into_iter().flatten().collect()),
            ]),
            3,
            ui.stack(vec![self.preview(ui), self.borders(ui)]),
        ));
        ui.panel(TITLE, "Changes show on your desktop as you drag", Some(segments), body)
    }

    fn summary(&self) -> String {
        if !self.options.ready() {
            return String::new();
        }
        let layout = self.options.text("general.layout");
        let label = LAYOUTS.iter().find(|(name, _)| *name == layout).map_or(layout, |l| l.1);
        format!("{label}, {:.0} px corners", self.options.number("decoration.rounding"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_colours_round_trip() {
        let color = parse("rgba(82dcccff)").unwrap();
        assert_eq!(format(color), "rgba(82dcccff)");
        assert_eq!(parse("red"), None);
    }

    #[test]
    fn editing_the_second_stop_extends_a_single_colour() {
        let value = Value::Gradient { colors: vec!["rgba(000000ff)".into()], angle: 45 };
        let edited = with_stop(&value, 1, Color::WHITE);
        assert_eq!(
            edited,
            Value::Gradient {
                colors: vec!["rgba(000000ff)".into(), "rgba(ffffffff)".into()],
                angle: 45
            }
        );
    }
}
