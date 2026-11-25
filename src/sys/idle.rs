//! Idle behaviour, stored in `hypridle.conf`.
//!
//! hypridle has no query interface, so the file is the state. It usually holds
//! hand-written commands, and those are kept: only the `timeout` of the listener
//! that performs an action is rewritten.

use std::path::PathBuf;

use super::{Change, Cmd, config_dir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Dim,
    Lock,
    ScreenOff,
    Suspend,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Dim, Stage::Lock, Stage::ScreenOff, Stage::Suspend];

    pub fn label(self) -> &'static str {
        match self {
            Stage::Dim => "Dim the screen",
            Stage::Lock => "Lock",
            Stage::ScreenOff => "Turn the screen off",
            Stage::Suspend => "Suspend",
        }
    }

    /// Recognises a listener by what its command does.
    fn matches(self, command: &str) -> bool {
        match self {
            Stage::Dim => command.contains("brightnessctl") && !command.contains("kbd"),
            Stage::Lock => command.contains("lock-session") || command.contains("hyprlock"),
            Stage::ScreenOff => command.contains("dpms") && command.contains("off"),
            Stage::Suspend => command.contains("suspend") || command.contains("hibernate"),
        }
    }

    /// The listener added when the file has none for this stage.
    fn listener(self, seconds: u32) -> String {
        let commands = match self {
            Stage::Dim => {
                "    on-timeout = brightnessctl -s set 10%\n    on-resume = brightnessctl -r\n"
            }
            Stage::Lock => "    on-timeout = loginctl lock-session\n",
            Stage::ScreenOff => {
                "    on-timeout = hyprctl dispatch 'hl.dsp.dpms(\"off\")'\n    \
                 on-resume = hyprctl dispatch 'hl.dsp.dpms(\"on\")'\n"
            }
            Stage::Suspend => "    on-timeout = systemctl suspend\n",
        };
        format!("\nlistener {{\n    timeout = {seconds}\n{commands}}}\n")
    }
}

/// One `listener { … }` block, as line numbers into the file.
struct Listener {
    start: usize,
    end: usize,
    timeout_line: Option<usize>,
    timeout: u32,
    on_timeout: String,
}

fn listeners(text: &str) -> Vec<Listener> {
    let mut found = Vec::new();
    let mut current: Option<Listener> = None;
    for (index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with("listener") && line.ends_with('{') {
            current = Some(Listener {
                start: index,
                end: index,
                timeout_line: None,
                timeout: 0,
                on_timeout: String::new(),
            });
        } else if let Some(listener) = &mut current {
            if line == "}" {
                listener.end = index;
                found.extend(current.take());
            } else if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "timeout" => {
                        listener.timeout_line = Some(index);
                        listener.timeout = value.trim().parse().unwrap_or(0);
                    }
                    "on-timeout" => listener.on_timeout = value.trim().to_owned(),
                    _ => {}
                }
            }
        }
    }
    found
}

/// Returns the timeout of `stage` in seconds, or `None` when it is off.
pub fn timeout(text: &str, stage: Stage) -> Option<u32> {
    listeners(text).into_iter().find(|l| stage.matches(&l.on_timeout)).map(|l| l.timeout)
}

/// Returns `text` with the timeout of `stage` replaced, added or removed.
pub fn with_timeout(text: &str, stage: Stage, seconds: Option<u32>) -> String {
    let listener = listeners(text).into_iter().find(|l| stage.matches(&l.on_timeout));
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    match (listener, seconds) {
        (Some(Listener { timeout_line: Some(line), .. }), Some(seconds)) => {
            let indent: String = lines[line].chars().take_while(|c| c.is_whitespace()).collect();
            lines[line] = format!("{indent}timeout = {seconds}");
        }
        (Some(listener), _) => {
            lines.drain(listener.start..=listener.end);
        }
        (None, Some(seconds)) => {
            return format!("{}\n{}", text.trim_end(), stage.listener(seconds));
        }
        (None, None) => {}
    }
    lines.join("\n") + "\n"
}

pub fn path() -> PathBuf {
    config_dir().join("hypr/hypridle.conf")
}

pub fn load() -> Option<String> {
    std::fs::read_to_string(path()).ok()
}

pub fn set(text: &str, stage: Stage, seconds: Option<u32>) -> Change {
    Change::write(path(), with_timeout(text, stage, seconds), vec![restart()])
}

/// hypridle reads its file only once, at startup.
pub fn restart() -> Cmd {
    Cmd::new("systemctl").args(["--user", "restart", "hypridle.service"])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = "general {\n    lock_cmd = pidof hyprlock || hyprlock\n}\n\
        listener {\n    timeout    = 240 # four minutes\n    on-timeout = brightnessctl -s set 10%\n}\n\
        listener {\n    timeout    = 360\n    on-timeout = loginctl lock-session\n}\n";

    #[test]
    fn finds_stages_by_their_command() {
        assert_eq!(timeout(CONF, Stage::Dim), Some(240));
        assert_eq!(timeout(CONF, Stage::Lock), Some(360));
        assert_eq!(timeout(CONF, Stage::Suspend), None);
    }

    #[test]
    fn edits_one_listener_and_leaves_the_rest() {
        let text = with_timeout(CONF, Stage::Lock, Some(600));
        assert_eq!(timeout(&text, Stage::Lock), Some(600));
        assert_eq!(timeout(&text, Stage::Dim), Some(240));
        assert!(text.contains("# four minutes"));

        let text = with_timeout(CONF, Stage::Dim, None);
        assert_eq!(timeout(&text, Stage::Dim), None);
        assert_eq!(timeout(&text, Stage::Lock), Some(360));

        let text = with_timeout(CONF, Stage::Suspend, Some(1800));
        assert_eq!(timeout(&text, Stage::Suspend), Some(1800));
        assert!(text.starts_with(CONF.trim_end()));
    }
}
