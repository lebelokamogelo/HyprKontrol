//! Every change this program makes to the system is a [`Cmd`] or a file write.
//!
//! An invocation is kept as data instead of being spawned on the spot. That is
//! what lets the receipt bar show the exact command before it runs, and lets a
//! change carry its own inverse for Undo.
//!
//! Arguments are passed as a vector, so SSIDs, paths and service names need no
//! escaping. [`Cmd::terminal`] is the only place where a shell is involved.

use std::fmt;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::{Error, Result};

/// Upper bound for commands that need no interaction.
///
/// Some clients block for good when their daemon is wedged, and the panel would
/// then wait forever. Elevated commands are exempt: they sit behind a polkit
/// prompt for as long as the user takes to answer it.
const TIMEOUT: Duration = Duration::from_secs(45);

const RETRY_DELAY: Duration = Duration::from_millis(400);

/// A program invocation that can be shown to the user before it is run.
#[derive(Clone, PartialEq, Eq)]
pub struct Cmd {
    program: String,
    args: Vec<String>,
    elevated: bool,
    detached: bool,
    stdin: Option<String>,
    attempts: u32,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            elevated: false,
            detached: false,
            stdin: None,
            attempts: 1,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<S: Into<String>>(mut self, args: impl IntoIterator<Item = S>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Runs the command as root.
    ///
    /// `pkexec` is used over `sudo` because it authenticates through the
    /// desktop's polkit agent and needs no terminal. It also clears the
    /// environment, so a command must not depend on inherited variables.
    pub fn root(mut self) -> Self {
        self.elevated = true;
        self
    }

    /// Starts the command and returns without waiting for it to exit.
    pub fn detached(mut self) -> Self {
        self.detached = true;
        self
    }

    /// Feeds `input` to the command's standard input.
    ///
    /// Use this for secrets. Anything in `args` is readable by every local user
    /// through `/proc/<pid>/cmdline` for as long as the command runs.
    pub fn stdin(mut self, input: impl Into<String>) -> Self {
        self.stdin = Some(input.into());
        self
    }

    /// Tries up to `attempts` times before giving up.
    ///
    /// For commands that depend on hardware the previous one has just woken.
    /// There is no event to wait for that works across drivers.
    pub fn retry(mut self, attempts: u32) -> Self {
        self.attempts = attempts;
        self
    }

    /// Opens `script` in a terminal emulator and leaves the window open until
    /// the user has read the result.
    ///
    /// Reserved for what cannot be shown in the window: typing a password, or
    /// reading build scripts from the AUR. Everything else that asks questions is
    /// a [`Session`](super::Session). `script` is interpreted by `sh`, so a caller
    /// that interpolates a value has to validate it first.
    pub fn terminal(script: impl fmt::Display) -> Self {
        let script = format!("{script}; printf '\\nPress Enter to close. '; read -r _");
        let emulator = ["kitty", "alacritty", "foot", "konsole", "ghostty", "xterm"]
            .into_iter()
            .find(|program| super::installed(program))
            .unwrap_or("xterm");
        let cmd = Cmd::new(emulator).detached();
        let cmd = match emulator {
            "kitty" | "foot" => cmd,
            _ => cmd.arg("-e"),
        };
        cmd.args(["sh", "-c", &script])
    }

    fn command(&self) -> Command {
        let mut command = if self.elevated {
            let mut command = Command::new("pkexec");
            command.arg(&self.program);
            command
        } else {
            Command::new(&self.program)
        };
        command
            .args(&self.args)
            // nmcli and the systemd tools translate their output, which would
            // break parsing. C.UTF-8 rather than C keeps non-ASCII names intact.
            .env("LC_ALL", "C.UTF-8")
            .stdin(if self.stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        command
    }

    /// Runs the command and returns its standard output.
    pub async fn output(&self) -> Result<String> {
        let mut result = self.attempt().await;
        for _ in 1..self.attempts {
            if result.is_ok() {
                break;
            }
            tokio::time::sleep(RETRY_DELAY).await;
            result = self.attempt().await;
        }
        result
    }

    async fn attempt(&self) -> Result<String> {
        if self.detached {
            self.command()
                .env_remove("LC_ALL")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| self.spawn_error(e))?;
            return Ok(String::new());
        }

        let mut child = self
            .command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| self.spawn_error(e))?;

        if let (Some(input), Some(mut pipe)) = (&self.stdin, child.stdin.take()) {
            pipe.write_all(input.as_bytes()).await?;
        }

        let wait = child.wait_with_output();
        let output = if self.elevated {
            wait.await?
        } else {
            tokio::time::timeout(TIMEOUT, wait)
                .await
                .map_err(|_| Error::new(format!("{} did not respond in time", self.program)))??
        };

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            return Ok(stdout);
        }
        // 126 means the user dismissed the prompt. That is a decision and not a
        // failure, so it gets its own wording instead of pkexec's error text.
        if self.elevated && output.status.code() == Some(126) {
            return Err(Error::new("Cancelled at the password prompt"));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = reason(&stderr, &stdout)
            .unwrap_or_else(|| format!("{} failed ({})", self.program, output.status));
        Err(Error::new(match self.program.as_str() {
            "bluetoothctl" => super::bluetooth::explain(&reason),
            _ => reason,
        }))
    }

    /// Runs the command, discarding its output.
    pub async fn run(&self) -> Result<()> {
        self.output().await.map(drop)
    }

    /// The program and arguments for running on a terminal.
    ///
    /// Without a polkit agent, `pkexec` would ask for the password on that
    /// terminal. It must not pass through this program, so that is turned off.
    pub fn in_terminal(&self) -> (&str, Vec<&str>) {
        let args = self.args.iter().map(String::as_str);
        if self.elevated {
            let own = ["--disable-internal-agent", self.program.as_str()];
            ("pkexec", own.into_iter().chain(args).collect())
        } else {
            (&self.program, args.collect())
        }
    }

    pub(super) fn spawn_error(&self, error: std::io::Error) -> Error {
        match error.kind() {
            std::io::ErrorKind::NotFound => {
                let program = if self.elevated { "pkexec" } else { &self.program };
                Error::new(format!("{program} is not installed"))
            }
            _ => error.into(),
        }
    }
}

/// Standard input is left out, since that is where secrets go.
impl fmt::Debug for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cmd({self})")
    }
}

/// Picks the line that says what went wrong.
///
/// Most tools put it last. nmcli follows it with a hint about the journal, which
/// would take its place, so a line that announces an error wins.
fn reason(stderr: &str, stdout: &str) -> Option<String> {
    let lines = || [stderr, stdout].into_iter().map(|text| text.lines().map(str::trim));
    let announced = lines().flatten().find(|line| line.starts_with("Error: "));
    let last = || lines().find_map(|lines| lines.rev().find(|line| !line.is_empty()));
    let line = announced.or_else(last)?;
    Some(line.trim_start_matches("Error: ").to_owned())
}

/// Formats the command the way it would be typed, so it can be copied into a shell.
impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.elevated {
            f.write_str("pkexec ")?;
        }
        f.write_str(&self.program)?;
        for arg in &self.args {
            let plain = !arg.is_empty()
                && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,~".contains(c));
            if plain {
                write!(f, " {arg}")?;
            } else {
                write!(f, " '{}'", arg.replace('\'', r"'\''"))?;
            }
        }
        Ok(())
    }
}

/// Shorthand for reads where a missing tool and a failed call mean the same
/// thing to the caller: the information is not available.
pub async fn query<const N: usize>(program: &str, args: [&str; N]) -> Option<String> {
    Cmd::new(program).args(args).output().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_only_when_needed() {
        let cmd = Cmd::new("nmcli").args(["connection", "up", "Home 5G"]);
        assert_eq!(cmd.to_string(), "nmcli connection up 'Home 5G'");
        let cmd = Cmd::new("ufw").arg("enable").root();
        assert_eq!(cmd.to_string(), "pkexec ufw enable");
        assert_eq!(Cmd::new("echo").arg("it's").to_string(), r"echo 'it'\''s'");
    }

    #[test]
    fn errors_win_over_hints() {
        let nmcli = "Error: Connection activation failed: Unknown reason\n\
                     Hint: use 'journalctl -xe' to get more details.\n";
        assert_eq!(reason(nmcli, "").unwrap(), "Connection activation failed: Unknown reason");
        assert_eq!(reason("", "first\nsecond\n\n").unwrap(), "second");
        assert_eq!(reason("\n", ""), None);
    }

    #[tokio::test]
    async fn reports_missing_program() {
        let error = Cmd::new("no-such-program-here").run().await.unwrap_err();
        assert_eq!(error.to_string(), "no-such-program-here is not installed");
    }

    #[tokio::test]
    async fn retries_until_it_succeeds() {
        let flag = std::env::temp_dir().join(format!("hyprkontrol-retry-{}", std::process::id()));
        let _ = std::fs::remove_file(&flag);
        // Fails once, leaving a file behind that makes the second attempt pass.
        let script = format!("test -e '{0}' || {{ touch '{0}'; exit 1; }}", flag.display());
        let cmd = Cmd::new("sh").args(["-c", &script]);
        assert!(cmd.clone().run().await.is_err());
        let _ = std::fs::remove_file(&flag);
        assert!(cmd.retry(2).run().await.is_ok());
        let _ = std::fs::remove_file(&flag);
    }

    #[tokio::test]
    async fn passes_stdin() {
        assert_eq!(Cmd::new("cat").stdin("hello").output().await.unwrap(), "hello");
    }
}
