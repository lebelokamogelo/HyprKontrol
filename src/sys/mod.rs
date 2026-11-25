//! Backends for the panels: one module per subsystem.
//!
//! Nothing below this point knows about the UI, so each module can be tested
//! without a display. Reads return plain data; writes are described as an
//! [`Op`] and executed by the application, which owns progress and Undo.

pub mod audio;
pub mod bluetooth;
pub mod cmd;
pub mod desktop;
pub mod edit;
pub mod hypr;
pub mod idle;
pub mod monitor;
pub mod network;
pub mod op;
pub mod packages;
pub mod power;
pub mod probe;
pub mod region;
pub mod rfkill;
pub mod services;
pub mod storage;
pub mod ufw;
pub mod vpn;

use std::fmt;
use std::path::PathBuf;

pub use cmd::{Cmd, query};
pub use op::{Change, Op};

/// A failure, already phrased for the user.
///
/// There is deliberately no error enum. Callers never branch on the cause: they
/// show the text next to the control that failed and leave the state untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self(format!("Unexpected data: {error}"))
    }
}

impl From<zbus::Error> for Error {
    fn from(error: zbus::Error) -> Self {
        match error {
            zbus::Error::MethodError(_, Some(text), _) => Self(text),
            other => Self(other.to_string()),
        }
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

/// `$XDG_CONFIG_HOME`, falling back to `~/.config`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home().join(".config"), PathBuf::from)
}

/// `$XDG_CACHE_HOME`, falling back to `~/.cache`.
pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home().join(".cache"), PathBuf::from)
}

/// Reports whether `program` is on `PATH`.
pub fn installed(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Reads a small text file, typically from `/proc` or `/sys`, without the trailing newline.
pub fn read(path: impl AsRef<std::path::Path>) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|text| text.trim().to_owned())
}

pub fn read_number(path: impl AsRef<std::path::Path>) -> Option<f64> {
    read(path)?.parse().ok()
}

/// Looks up `key` in `KEY=value` text, stripping optional quotes.
pub fn lookup<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches('"'))
    })
}

/// Formats a byte count. Multiples are binary but labelled KB, MB and so on, matching `df -h`.
pub fn bytes(count: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = count as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 10.0 || unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(499_000_000_000), "465 GB");
    }

    #[test]
    fn looks_up_quoted_values() {
        let text = "A=1\nDEFAULT_INPUT_POLICY=\"DROP\"\n";
        assert_eq!(lookup(text, "DEFAULT_INPUT_POLICY"), Some("DROP"));
        assert_eq!(lookup(text, "B"), None);
    }
}
