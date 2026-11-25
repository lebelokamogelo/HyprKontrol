//! Notices when volume or brightness change, whoever changes them.
//!
//! Keys on the keyboard change them too, and a slider that trails behind looks
//! broken. Both are announced by the system, so nothing has to be polled.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::time::{Instant, timeout_at};

/// A key that is held down repeats. Changes that follow each other more closely
/// than this are reported together.
const PACE: Duration = Duration::from_millis(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Volume,
    Brightness,
}

impl Source {
    fn command(self) -> &'static [&'static str] {
        match self {
            Source::Volume => &["pactl", "subscribe"],
            Source::Brightness => {
                &["udevadm", "monitor", "--kernel", "--subsystem-match=backlight"]
            }
        }
    }

    /// Reading the volume is itself done by a client that comes and goes. Taking
    /// that for a change would lead to reading it again without end.
    fn announces(self, line: &str) -> bool {
        match self {
            Source::Volume => line.starts_with("Event ") && !line.contains(" on client "),
            Source::Brightness => line.starts_with("KERNEL["),
        }
    }
}

pub struct Watcher {
    source: Source,
    lines: Lines<BufReader<ChildStdout>>,
    reported: Option<Instant>,
    /// Ends the command when dropped.
    _child: Child,
}

impl Watcher {
    /// `None` if the tool is missing.
    pub fn start(source: Source) -> Option<Self> {
        let (program, args) = source.command().split_first()?;
        let mut child = Command::new(program)
            .args(args)
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let lines = BufReader::new(child.stdout.take()?).lines();
        Some(Self { source, lines, reported: None, _child: child })
    }

    /// Waits for a change. Returns `false` once there will be no more of them.
    ///
    /// The first change is reported at once and the last of a series always is.
    pub async fn changed(&mut self) -> bool {
        loop {
            match self.lines.next_line().await {
                Ok(Some(line)) if self.source.announces(&line) => break,
                Ok(Some(_)) => {}
                _ => return false,
            }
        }
        if let Some(reported) = self.reported {
            let next = reported + PACE;
            while let Ok(Ok(Some(_))) = timeout_at(next, self.lines.next_line()).await {}
        }
        self.reported = Some(Instant::now());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_the_volume_is_not_a_change() {
        let volume = |line| Source::Volume.announces(line);
        assert!(volume("Event 'change' on sink #63"));
        assert!(volume("Event 'change' on source #64"));
        assert!(volume("Event 'change' on server #-1"));
        assert!(volume("Event 'new' on sink-input #80"));
        assert!(!volume("Event 'new' on client #2990"));
        assert!(!volume("Got SIGINT, exiting."));
    }

    #[test]
    fn only_events_count_as_brightness() {
        let brightness = |line| Source::Brightness.announces(line);
        assert!(brightness(
            "KERNEL[48193.120627] change   /devices/pci0000:00/0000:00:02.0/drm/card1/card1-eDP-1/intel_backlight (backlight)"
        ));
        assert!(!brightness("monitor will print the received events for:"));
        assert!(!brightness("KERNEL - the kernel uevent"));
        assert!(!brightness(""));
    }

    /// Feeds a watcher from a script instead of the system.
    fn scripted(script: &str) -> Watcher {
        let mut child = Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        Watcher { source: Source::Volume, lines, reported: None, _child: child }
    }

    #[tokio::test]
    async fn reports_the_first_change_at_once_and_bundles_the_rest() {
        let mut watcher = scripted(
            "echo \"Event 'new' on client #1\"; echo \"Event 'change' on sink #1\"; sleep 0.4; \
             for i in 1 2 3 4 5 6 7 8; do echo \"Event 'change' on sink #1\"; done; sleep 0.4",
        );
        let started = Instant::now();
        assert!(watcher.changed().await);
        assert!(started.elapsed() < Duration::from_millis(350));
        let mut reports = 0;
        while watcher.changed().await {
            reports += 1;
        }
        assert!((1..=2).contains(&reports), "{reports}");
    }

    /// Prints the time from a change to its new value. Leaves the volume as it is.
    #[tokio::test]
    #[ignore = "needs a running desktop"]
    async fn follows_the_volume() {
        use crate::sys::audio::{self, Direction};

        let mut watcher = Watcher::start(Source::Volume).unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let before = audio::load().await.unwrap();
        let speaker = before.output().unwrap().clone();
        let set = |percent| audio::set_volume(Direction::Output, &speaker.name, percent);
        for percent in [(speaker.volume - 1.0).max(0.0), speaker.volume] {
            let started = Instant::now();
            set(percent).run().await.unwrap();
            assert!(watcher.changed().await);
            let now = audio::load().await.unwrap().output().unwrap().volume;
            println!("{now:.0}% after {} ms", started.elapsed().as_millis());
            assert_eq!(now.round(), percent.round());
            // Reading it must not count as another change.
            let quiet = tokio::time::timeout(Duration::from_secs(1), watcher.changed()).await;
            let quiet = quiet.is_err()
                || tokio::time::timeout(Duration::from_secs(1), watcher.changed()).await.is_err();
            assert!(quiet, "the watcher wakes itself");
        }
    }
}
