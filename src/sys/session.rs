//! Commands that talk to the user while they run.
//!
//! pacman only draws progress, and only asks its questions, on a terminal. A
//! [`Session`] gives it one without opening a window: the program runs in a
//! pseudo-terminal, what it prints is handed over piece by piece, and answers
//! are written back as if they had been typed.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout, timeout_at};

use super::{Cmd, Error, Result};

/// Wide enough that pacman neither shortens file names nor drops its package
/// table, which it does when a column does not fit.
const SIZE: (u16, u16) = (50, 160);

/// Ctrl+C. The terminal delivers it as an interrupt to whatever runs in it,
/// which reaches a program running as root, where a signal would be refused.
pub const INTERRUPT: &str = "\x03";

/// pacman repaints every bar several times a second. Output that arrives within
/// this time is delivered in one piece.
const GATHER: Duration = Duration::from_millis(100);

/// How long to wait for the rest of the output once the program has exited. A
/// daemon started by a hook can keep the terminal open for good.
const DRAIN: Duration = Duration::from_millis(250);

/// A command to run in front of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub cmd: Cmd,
    /// What it does, such as "Ranking the mirrors".
    pub doing: String,
    /// What it has done, such as "Mirrors ranked".
    pub done: String,
    /// It reports no progress that could be drawn, so its output is shown.
    pub plain: bool,
}

impl Session {
    pub fn new(cmd: Cmd, doing: impl Into<String>, done: impl Into<String>) -> Self {
        Self { cmd, doing: doing.into(), done: done.into(), plain: false }
    }

    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Output(Vec<u8>),
    /// The exit code, or `None` if a signal ended the program.
    Exited(Option<i32>),
    /// It could not be started.
    Failed(Error),
}

/// Types into a running session.
pub type Keys = mpsc::UnboundedSender<String>;

pub fn keys() -> (Keys, mpsc::UnboundedReceiver<String>) {
    mpsc::unbounded_channel()
}

/// A session under way.
pub struct Running {
    pty: pty_process::Pty,
    child: Child,
    keys: mpsc::UnboundedReceiver<String>,
    exited: Option<Option<i32>>,
}

impl Running {
    pub fn start(cmd: &Cmd, keys: mpsc::UnboundedReceiver<String>) -> Result<Self> {
        let failed = |error: pty_process::Error| Error::new(format!("No terminal: {error}"));
        let (pty, pts) = pty_process::open().map_err(failed)?;
        pty.resize(pty_process::Size::new(SIZE.0, SIZE.1)).map_err(failed)?;
        let (program, args) = cmd.in_terminal();
        let child = pty_process::Command::new(program)
            .args(args)
            .env("LC_ALL", "C.UTF-8")
            .env("TERM", "dumb")
            .spawn(pts)
            .map_err(|error| match error {
                pty_process::Error::Io(error) => cmd.spawn_error(error),
                error => failed(error),
            })?;
        Ok(Self { pty, child, keys, exited: None })
    }

    /// Waits for the next thing to happen. Nothing follows [`Event::Exited`].
    pub async fn next(&mut self) -> Event {
        let mut buffer = vec![0; 16 * 1024];
        loop {
            if let Some(code) = self.exited {
                return match timeout(DRAIN, self.pty.read(&mut buffer)).await {
                    Ok(Ok(count)) if count > 0 => Event::Output(buffer[..count].to_vec()),
                    _ => Event::Exited(code),
                };
            }
            tokio::select! {
                read = self.pty.read(&mut buffer) => match read {
                    Ok(count) if count > 0 => {
                        let mut output = buffer[..count].to_vec();
                        let deadline = Instant::now() + GATHER;
                        while let Ok(Ok(count)) = timeout_at(deadline, self.pty.read(&mut buffer)).await
                            && count > 0
                        {
                            output.extend_from_slice(&buffer[..count]);
                        }
                        return Event::Output(output);
                    }
                    // Linux reports an error once nothing has the terminal open.
                    _ => {
                        let code = self.child.wait().await.ok().and_then(|status| status.code());
                        self.exited = Some(code);
                        return Event::Exited(code);
                    }
                },
                status = self.child.wait() => {
                    self.exited = Some(status.ok().and_then(|status| status.code()));
                }
                Some(text) = self.keys.recv() => {
                    // A program that is gone has no use for the answer.
                    let _ = self.pty.write_all(text.as_bytes()).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn transcript(script: &str, answers: &[&str]) -> (String, Event) {
        let (keys, receiver) = keys();
        let cmd = Cmd::new("sh").args(["-c", script]);
        let mut running = Running::start(&cmd, receiver).unwrap();
        let mut answers = answers.iter();
        let mut text = String::new();
        loop {
            match running.next().await {
                Event::Output(bytes) => {
                    text.push_str(&String::from_utf8_lossy(&bytes));
                    if text.ends_with("? ")
                        && let Some(answer) = answers.next()
                    {
                        keys.send((*answer).to_owned()).unwrap();
                    }
                }
                end => return (text, end),
            }
        }
    }

    #[tokio::test]
    async fn runs_on_a_terminal_and_reports_the_exit_code() {
        let (text, end) = transcript("test -t 0 && test -t 1 && stty size; exit 3", &[]).await;
        assert_eq!(text.trim(), "50 160");
        assert_eq!(end, Event::Exited(Some(3)));
    }

    #[tokio::test]
    async fn answers_questions() {
        let script = "printf 'Proceed? '; read -r answer; echo \"got $answer\"";
        let (text, end) = transcript(script, &["y\n"]).await;
        assert!(text.contains("got y"), "{text}");
        assert_eq!(end, Event::Exited(Some(0)));
    }

    #[tokio::test]
    async fn interrupts() {
        let script = "trap 'exit 130' INT; printf 'Busy? '; while :; do sleep 0.05; done";
        let (_, end) = transcript(script, &[INTERRUPT]).await;
        assert_eq!(end, Event::Exited(Some(130)));
    }

    #[tokio::test]
    async fn does_not_wait_for_daemons_left_behind() {
        let started = std::time::Instant::now();
        let (text, end) = transcript("(trap '' HUP; sleep 8) & echo done", &[]).await;
        assert_eq!(text.trim(), "done");
        assert_eq!(end, Event::Exited(Some(0)));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[tokio::test]
    async fn reports_a_missing_program() {
        let (_, receiver) = keys();
        let error = Running::start(&Cmd::new("no-such-program-here"), receiver).err().unwrap();
        assert_eq!(error.to_string(), "no-such-program-here is not installed");
    }
}
