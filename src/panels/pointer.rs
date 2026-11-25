use iced::Task;

use super::options::{OptionMessage, Options};
use super::{Effect, Panel};
use crate::sys::hypr::Value;
use crate::ui::{Element, Ui};

const KEYS: &[&str] = &[
    "input.sensitivity",
    "input.accel_profile",
    "input.natural_scroll",
    "input.scroll_factor",
    "input.left_handed",
    "input.follow_mouse",
    "input.touchpad.natural_scroll",
    "input.touchpad.scroll_factor",
    "input.touchpad.tap_to_click",
    "input.touchpad.tap_and_drag",
    "input.touchpad.disable_while_typing",
    "input.touchpad.clickfinger_behavior",
    "input.touchpad.middle_button_emulation",
];

#[derive(Default)]
pub struct State {
    options: Options,
}

#[derive(Debug, Clone)]
pub enum Message {
    Option(OptionMessage),
    Acceleration(&'static str),
    Focus(&'static str),
}

const ACCELERATION: [(&str, &str); 3] =
    [("", "Device default"), ("adaptive", "Adaptive"), ("flat", "Flat")];

const FOCUS: [(&str, &str); 4] = [
    ("0", "Click to focus"),
    ("1", "Follow the pointer"),
    ("2", "Follow, keep keyboard"),
    ("3", "Separate"),
];

impl Panel for State {
    type Message = Message;

    fn load(&self) -> Task<Message> {
        Options::load(KEYS).map(Message::Option)
    }

    fn update(&mut self, message: Message) -> Effect<Message> {
        let message = match message {
            Message::Option(message) => message,
            Message::Acceleration(profile) => {
                OptionMessage::Set("input.accel_profile", Value::Text(profile.to_owned()))
            }
            Message::Focus(mode) => OptionMessage::Set(
                "input.follow_mouse",
                Value::Int(mode.parse().expect("modes are numbers")),
            ),
        };
        self.options.update(message).map(Message::Option)
    }

    fn view<'a>(&'a self, ui: Ui<'a>) -> Element<'a, Message> {
        const TITLE: &str = "Mouse and touchpad";
        if !self.options.ready() {
            return ui.panel(TITLE, "", None, vec![ui.loading()]);
        }
        let o = &self.options;
        let toggle = |key, label, hint| o.toggle(ui, key, label, hint, Message::Option);
        let factor = |key| {
            o.slide(
                ui,
                key,
                "Scroll speed",
                0.2..=3.0,
                0.1,
                |value| format!("{value:.1}×"),
                Message::Option,
            )
        };

        let mut mouse = Vec::new();
        mouse.extend(o.slide(
            ui,
            "input.sensitivity",
            "Pointer speed",
            -1.0..=1.0,
            0.05,
            |value| format!("{value:+.2}"),
            Message::Option,
        ));
        if o.has("input.accel_profile") {
            let profile = o.text("input.accel_profile");
            mouse.push(ui.row(
                "Acceleration",
                "Flat moves the pointer exactly as far as your hand",
                ui.choose(&ACCELERATION, profile, Message::Acceleration),
            ));
        }
        mouse.extend(factor("input.scroll_factor"));
        mouse.extend(toggle(
            "input.natural_scroll",
            "Natural scrolling",
            "Content follows the wheel",
        ));
        mouse.extend(toggle("input.left_handed", "Left-handed", "Swaps the main buttons"));
        if o.has("input.follow_mouse") {
            let mode = format!("{:.0}", o.number("input.follow_mouse"));
            mouse.push(ui.row("Window focus", "", ui.choose(&FOCUS, &mode, Message::Focus)));
        }

        let mut touchpad = Vec::new();
        touchpad.extend(toggle("input.touchpad.tap_to_click", "Tap to click", ""));
        touchpad.extend(toggle("input.touchpad.tap_and_drag", "Tap and drag", ""));
        touchpad.extend(toggle(
            "input.touchpad.natural_scroll",
            "Natural scrolling",
            "Content follows your fingers",
        ));
        touchpad.extend(factor("input.touchpad.scroll_factor"));
        touchpad.extend(toggle(
            "input.touchpad.disable_while_typing",
            "Ignore while typing",
            "Prevents accidental clicks with the palm",
        ));
        touchpad.extend(toggle(
            "input.touchpad.clickfinger_behavior",
            "Click with two fingers for the right button",
            "Otherwise the corner of the touchpad decides",
        ));
        touchpad.extend(toggle(
            "input.touchpad.middle_button_emulation",
            "Both buttons for the middle button",
            "",
        ));

        ui.panel(
            TITLE,
            "",
            None,
            vec![ui.columns(ui.group("Mouse", mouse), ui.group("Touchpad", touchpad))],
        )
    }

    fn summary(&self) -> String {
        match self.options.text("input.accel_profile") {
            "flat" => "Flat acceleration".to_owned(),
            "adaptive" => "Adaptive acceleration".to_owned(),
            _ if self.options.ready() => "Device default".to_owned(),
            _ => String::new(),
        }
    }
}
