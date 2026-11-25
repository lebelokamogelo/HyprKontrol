//! Follows a package transaction through what pacman prints.
//!
//! pacman has no machine-readable mode, and its library would have to run as
//! root inside this program. So the terminal output is read instead. Anything
//! that is not recognised is still shown word for word, and a question that is
//! not recognised can be answered by typing, so a change in pacman's wording
//! costs comfort and never control.

use super::screen::Screen;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Stage {
    /// Nothing printed yet, which means the password prompt is open.
    #[default]
    Waiting,
    /// Package lists, dependencies and questions.
    Preparing,
    Downloading,
    /// Signatures, file conflicts and disk space.
    Checking,
    /// From here on, stopping leaves the system half changed.
    Installing,
    /// Hooks, such as rebuilding boot images.
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Package {
    pub name: String,
    /// Installed version. `None` if the package is new.
    pub from: Option<String>,
    /// Version to install. `None` if the package is being removed.
    pub to: Option<String>,
    /// Download size in MiB, if it is known.
    pub size: Option<f32>,
    /// MiB downloaded so far.
    pub got: f32,
    /// How much has been downloaded, from 0 to 1.
    pub fetched: f32,
    /// How much has been installed or removed, from 0 to 1.
    pub applied: f32,
    pub failed: bool,
}

impl Package {
    /// Whether it is being removed, as opposed to not listed by pacman.
    pub fn leaves(&self) -> bool {
        self.from.is_some() && self.to.is_none()
    }
}

/// What pacman is working on, such as `(2/4) upgrading bat`.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub index: usize,
    pub count: usize,
    pub label: String,
    pub fraction: Option<f32>,
}

/// All downloads together.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Progress {
    pub fraction: f32,
    pub got: f32,
    /// As printed, such as `892 KiB/s`.
    pub speed: String,
    /// Seconds left. `None` while pacman has no estimate.
    pub left: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub number: String,
    pub name: String,
    /// The repository it comes from.
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// With pacman's own suggestion.
    YesNo(bool),
    /// With the number pacman takes if none is given.
    Number(Vec<Choice>, String),
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// The question as printed.
    pub prompt: String,
    /// Lines above it that belong to it.
    pub context: Vec<String>,
    pub answer: Answer,
}

impl Question {
    /// Asks to go ahead with the transaction as a whole.
    pub fn is_proceed(&self) -> bool {
        self.prompt.starts_with("Proceed with") || self.is_removal()
    }

    pub fn is_removal(&self) -> bool {
        self.prompt.starts_with("Do you want to remove these packages?")
    }

    /// The question without the list of answers.
    pub fn text(&self) -> &str {
        let text = self.prompt.trim_end_matches(':').trim_end();
        text.trim_end_matches("[Y/n]").trim_end_matches("[y/N]").trim_end()
    }
}

/// Why a transaction did not go through, phrased for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub title: String,
    pub hint: String,
    /// Faster or working mirrors would help.
    pub mirrors: bool,
}

#[derive(Debug, Default)]
pub struct Transaction {
    screen: Screen,
    pub stage: Stage,
    pub packages: Vec<Package>,
    pub step: Option<Step>,
    pub progress: Progress,
    pub question: Option<Question>,
    /// The latest line a hook or a check has printed.
    pub detail: String,
    pub download_size: Option<String>,
    pub installed_size: Option<String>,
    pub removed_size: Option<String>,
    pub net_size: Option<String>,
    /// Packages as `(verb, name)`, in the order they were changed.
    pub changes: Vec<(String, String)>,
    /// A hook has asked for a restart.
    pub restart: bool,
    /// First row of the transaction being shown. Tools such as chwd run several.
    start: usize,
}

impl Transaction {
    pub fn feed(&mut self, bytes: &[u8]) {
        let touched = self.screen.feed(bytes);
        if self.stage == Stage::Waiting && !touched.is_empty() {
            self.stage = Stage::Preparing;
        }
        for row in touched {
            let line = self.screen.line(row);
            self.observe(row, &line);
        }
        self.question = self.screen.open_line().and_then(|line| self.recognise(&line));
    }

    /// Treats the line the cursor is on as a question.
    ///
    /// Called when nothing has been printed for a while. Scripts ask in ways
    /// that cannot be told from ordinary output, other than by waiting.
    pub fn suspect(&mut self) {
        if self.question.is_some() || self.stage == Stage::Waiting {
            return;
        }
        if let Some(line) = self.screen.open_line()
            && line.ends_with([':', '?', ']', '>', ')'])
            && split_bar(&line).1.is_none()
        {
            let context = self.context(&line);
            let prompt = line.trim().trim_start_matches(":: ").to_owned();
            self.question = Some(Question { prompt, context, answer: Answer::Text });
        }
    }

    /// Whether interrupting now leaves everything as it was.
    pub fn can_stop(&self) -> bool {
        self.stage < Stage::Installing
    }

    /// The output, with progress bars reduced to their percentage.
    pub fn output(&self, limit: usize) -> String {
        let first = self.screen.len().saturating_sub(limit);
        let lines: Vec<_> = (first..self.screen.len())
            .map(|row| {
                let line = self.screen.line(row);
                match split_bar(&line) {
                    (text, Some(percent)) => format!("{}  {percent}%", squeeze(text)),
                    _ => line,
                }
            })
            .collect();
        lines.join("\n").trim_end().to_owned()
    }

    /// What was said about problems, without the "warning: " in front.
    pub fn warnings(&self) -> Vec<String> {
        self.messages("warning: ")
    }

    pub fn errors(&self) -> Vec<String> {
        self.messages("error: ")
    }

    fn messages(&self, prefix: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for row in 0..self.screen.len() {
            if let Some(text) = self.screen.line(row).strip_prefix(prefix)
                && !found.iter().any(|known| known == text)
            {
                found.push(text.to_owned());
            }
        }
        found
    }

    /// What the transaction has done, such as "4 packages updated".
    pub fn summary(&self) -> Option<String> {
        let groups: [(&[&str], &str); 3] = [
            (&["upgrading", "downgrading"], "updated"),
            (&["installing", "reinstalling"], "installed"),
            (&["removing"], "removed"),
        ];
        let mut parts = Vec::new();
        for (verbs, done) in groups {
            let names: Vec<_> = self
                .changes
                .iter()
                .filter(|(verb, _)| verbs.contains(&verb.as_str()))
                .map(|(_, name)| name.as_str())
                .collect();
            match names.as_slice() {
                [] => {}
                [name] if parts.is_empty() => parts.push(format!("{name} {done}")),
                names if parts.is_empty() => {
                    parts.push(format!("{} packages {done}", names.len()));
                }
                names => parts.push(format!("{} {done}", names.len())),
            }
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    pub fn has_nothing_to_do(&self) -> bool {
        self.changes.is_empty()
            && (0..self.screen.len()).any(|row| self.screen.line(row) == " there is nothing to do")
    }

    /// Explains a failure. `code` is the exit code.
    pub fn failure(&self, code: Option<i32>) -> Failure {
        let failure = |title: &str, hint: &str| Failure {
            title: title.to_owned(),
            hint: hint.to_owned(),
            mirrors: false,
        };
        let errors = self.errors();
        let find = |needle: &str| errors.iter().find(|error| error.contains(needle));

        if find("unable to lock database").is_some() || find("could not lock database").is_some() {
            return failure(
                "Another package manager is running",
                "Wait until it is done, then try again. If nothing else is running, a past one \
                 was cut off and left /var/lib/pacman/db.lck behind.",
            );
        }
        if let Some(error) = find("failed retrieving file") {
            let file = error.split('\'').nth(1).unwrap_or_default();
            let name = package_of(file).map_or(file, |(name, _)| name);
            let mirror = error.split(" from ").nth(1).and_then(|rest| rest.split(" : ").next());
            let mirror = mirror.unwrap_or("The mirror").trim();
            return Failure {
                title: format!("{name} could not be downloaded"),
                hint: format!(
                    "{mirror} did not deliver it. Finished downloads are kept for the next try."
                ),
                mirrors: true,
            };
        }
        if let Some(error) = find("target not found: ") {
            let name = error.rsplit(": ").next().unwrap_or_default();
            return failure(
                &format!("{name} was not found"),
                "It is not in any repository pacman knows. Updating the system refreshes its lists.",
            );
        }
        if find("could not satisfy dependencies").is_some() {
            let reason = (0..self.screen.len())
                .map(|row| self.screen.line(row))
                .find(|line| line.starts_with(":: ") && line.contains("dependency"));
            let hint = reason.as_deref().map_or("", |line| line.trim_start_matches(":: "));
            return failure("Other packages depend on this", hint);
        }
        if find("not enough free disk space").is_some() || find("No space left").is_some() {
            return failure("The disk is full", "Free some space in Storage, then try again.");
        }
        if find("conflicting files").is_some() {
            return failure(
                "Files are in the way",
                "pacman does not replace files it did not install. The output lists them.",
            );
        }
        if find("invalid or corrupted package").is_some() {
            return failure(
                "A package did not pass its check",
                "It was damaged on the way or signed with a key that is not trusted.",
            );
        }
        let denied = (0..self.screen.len()).map(|row| self.screen.line(row)).find_map(|line| {
            line.strip_prefix("Error executing command as another user: ").map(str::to_owned)
        });
        if let Some(reason) = denied {
            return failure("No permission", &reason);
        }
        match errors.first() {
            Some(error) => failure(&capitalise(error), "The output has the details."),
            None => {
                let end = match code {
                    Some(code) => format!("It ended with code {code}. "),
                    None => "It was ended from outside. ".to_owned(),
                };
                failure("This did not work", &format!("{end}The output has the details."))
            }
        }
    }

    fn observe(&mut self, row: usize, line: &str) {
        let (text, percent) = split_bar(line);
        let text = text.trim_end();
        let fraction = percent.map(|percent| f32::from(percent) / 100.0);

        match text.trim() {
            "resolving dependencies..." | "checking dependencies..." => {
                if self.stage >= Stage::Installing {
                    self.begin(row);
                }
                self.detail = "Resolving dependencies".to_owned();
            }
            "looking for conflicting packages..." => {
                self.detail = "Looking for conflicts".to_owned();
            }
            ":: Synchronizing package databases..." => {
                self.detail = "Refreshing package lists".to_owned();
            }
            ":: Retrieving packages..." => self.enter(Stage::Downloading),
            ":: Running pre-transaction hooks..." => self.enter(Stage::Checking),
            ":: Processing package changes..." => self.enter(Stage::Installing),
            ":: Running post-transaction hooks..." => self.enter(Stage::Finishing),
            text if text.contains("Reboot is recommended") => self.restart = true,
            _ => {}
        }

        let sizes = [
            ("Total Download Size:", &mut self.download_size),
            ("Total Installed Size:", &mut self.installed_size),
            ("Total Removed Size:", &mut self.removed_size),
            ("Net Upgrade Size:", &mut self.net_size),
        ];
        for (label, size) in sizes {
            if let Some(value) = text.strip_prefix(label) {
                *size = Some(value.trim().to_owned());
                return;
            }
        }

        if let Some((index, count, label)) = numbered(text) {
            self.advance(Step { index, count, label: label.to_owned(), fraction });
        } else if self.stage == Stage::Downloading && text.starts_with(' ') {
            self.download(text.trim(), fraction);
        } else if self.stage == Stage::Finishing && !text.trim().is_empty() {
            self.detail =
                text.trim().trim_start_matches("==> ").trim_start_matches("-> ").to_owned();
        }
    }

    /// Starts over for another transaction of the same program. What has been
    /// changed so far stays on record.
    fn begin(&mut self, row: usize) {
        self.stage = Stage::Preparing;
        self.packages.clear();
        self.step = None;
        self.progress = Progress::default();
        self.download_size = None;
        self.installed_size = None;
        self.removed_size = None;
        self.net_size = None;
        self.start = row;
    }

    fn enter(&mut self, stage: Stage) {
        if stage > self.stage {
            self.stage = stage;
            self.step = None;
            self.detail.clear();
        }
        self.list();
    }

    fn advance(&mut self, step: Step) {
        const CHECKS: [&str; 5] = [
            "checking keys in keyring",
            "checking package integrity",
            "loading package files",
            "checking for file conflicts",
            "checking available disk space",
        ];
        const VERBS: [&str; 5] =
            ["upgrading", "installing", "reinstalling", "downgrading", "removing"];

        if CHECKS.contains(&step.label.as_str()) {
            self.enter(Stage::Checking);
        } else if self.stage <= Stage::Installing
            && let Some((verb, name)) = step.label.split_once(' ')
            && VERBS.contains(&verb)
        {
            // The bar comes last. Without it, the name may not be complete yet.
            if step.fraction.is_none() {
                return;
            }
            let name = name.trim().to_owned();
            let verb = verb.to_owned();
            self.enter(Stage::Installing);
            // One at a time, so whatever comes before this is finished.
            for package in &mut self.packages {
                if package.applied > 0.0 && package.name != name {
                    package.applied = 1.0;
                }
            }
            let package = self.package(&name);
            package.applied = step.fraction.unwrap_or(0.0).max(0.001);
            if !self.changes.iter().any(|(_, known)| *known == name) {
                self.changes.push((verb, name));
            }
        } else if self.stage < Stage::Checking {
            return;
        }
        self.step = Some(step);
    }

    /// Reads a line such as ` bat-0.26.1-3.1-x86_64_v3  8.4 MiB  219 KiB/s 11:07`.
    fn download(&mut self, text: &str, fraction: Option<f32>) {
        if let Some(file) = text.strip_suffix(" failed to download") {
            let name = package_of(file.trim()).map_or("", |(name, _)| name);
            for package in self.packages.iter_mut().filter(|package| package.name == name) {
                package.failed = true;
            }
            return;
        }
        let words: Vec<_> = text.split_whitespace().collect();
        let (name, status) = match words.as_slice() {
            [name @ .., got, unit, speed, per, left] if per.ends_with("/s") => {
                let got = mib(got, unit).unwrap_or(0.0);
                (name.join(" "), Some((got, format!("{speed} {per}"), seconds(left))))
            }
            _ => (text.to_owned(), None),
        };
        let fraction = fraction.unwrap_or(0.0);
        if name.starts_with("Total (") {
            if let Some((got, speed, left)) = status {
                // At the end, the time is how long it took.
                let left = left.filter(|_| fraction < 1.0);
                self.progress = Progress { fraction, got, speed, left };
            }
            return;
        }
        // A line that is only half printed can look like another package, so it
        // has to name a listed one, in the right version.
        let Some((got, speed, left)) = status else { return };
        let Some((name, version)) = package_of(&name) else { return };
        let listed = self.packages.iter().position(|package| {
            package.name == name && package.to.as_deref().is_none_or(|to| to == version)
        });
        let Some(index) = listed else { return };
        let package = &mut self.packages[index];
        package.got = got;
        package.fetched = fraction;
        if package.size.is_none() && fraction > 0.01 {
            package.size = Some(got / fraction);
        }
        // pacman leaves the total out when there is only one file.
        if self.packages.len() == 1 {
            let left = left.filter(|_| fraction < 1.0);
            self.progress = Progress { fraction, got, speed, left };
        }
    }

    fn package(&mut self, name: &str) -> &mut Package {
        let index = self.packages.iter().position(|package| package.name == name);
        let index = index.unwrap_or_else(|| {
            self.packages.push(Package { name: name.to_owned(), ..Package::default() });
            self.packages.len() - 1
        });
        &mut self.packages[index]
    }

    /// Reads the list of packages, once it is complete.
    fn list(&mut self) {
        if !self.packages.is_empty() {
            return;
        }
        let lines: Vec<_> =
            (self.start..self.screen.len()).map(|row| self.screen.line(row)).collect();
        self.packages = parse_list(&lines);
    }

    fn recognise(&mut self, line: &str) -> Option<Question> {
        let prompt = line.trim().trim_start_matches(":: ").to_owned();
        let answer = if prompt.ends_with("[Y/n]") {
            Answer::YesNo(true)
        } else if prompt.ends_with("[y/N]") {
            Answer::YesNo(false)
        } else if let Some(rest) = prompt.strip_prefix("Enter a number (default=") {
            let default = rest.trim_end_matches(':').trim_end_matches(')').to_owned();
            Answer::Number(self.choices(), default)
        } else if prompt.starts_with("Enter a selection (default=") && prompt.ends_with(':') {
            Answer::Text
        } else {
            return None;
        };
        self.list();
        let context = self.context(line);
        Some(Question { prompt, context, answer })
    }

    /// The paragraph that leads up to the question.
    fn context(&self, question: &str) -> Vec<String> {
        const LIMIT: usize = 12;
        let end = self.screen.len().saturating_sub(1);
        let mut lines: Vec<String> = Vec::new();
        for row in (self.start..end).rev() {
            let line = self.screen.line(row);
            // The list of choices is followed by a blank line.
            if line.trim().is_empty() && lines.is_empty() {
                continue;
            }
            let ends = line.trim().is_empty()
                || split_bar(&line).1.is_some()
                || line.ends_with("...")
                || line.starts_with("Total ")
                || line.starts_with("Net ")
                || line.starts_with("Enter a ")
                || line.contains("[Y/n] ")
                || line.contains("[y/N] ")
                || lines.len() == LIMIT;
            if ends || line == question {
                break;
            }
            lines.push(line);
        }
        lines.reverse();
        lines
    }

    /// Reads `   1) ncurses  2) other` and the repositories they are under.
    fn choices(&self) -> Vec<Choice> {
        let mut choices = Vec::new();
        let mut origin = String::new();
        for line in self.context("") {
            if let Some(name) = line.strip_prefix(":: Repository ") {
                name.clone_into(&mut origin);
                continue;
            }
            for item in line.split("  ") {
                if let Some((number, name)) = item.trim().split_once(") ")
                    && number.chars().all(|c| c.is_ascii_digit())
                    && !number.is_empty()
                {
                    choices.push(Choice {
                        number: number.to_owned(),
                        name: name.trim().to_owned(),
                        origin: origin.clone(),
                    });
                }
            }
        }
        choices
    }
}

/// Separates a line from the progress bar at its end.
fn split_bar(line: &str) -> (&str, Option<u8>) {
    let bar = || {
        let (text, percent) = line.trim_end().strip_suffix('%')?.rsplit_once(']')?;
        let percent = percent.trim().parse::<u8>().ok().filter(|percent| *percent <= 100)?;
        Some((text.rsplit_once('[')?.0, percent))
    };
    bar().map_or((line, None), |(text, percent)| (text, Some(percent)))
}

/// Parses `(2/4) upgrading bat`.
fn numbered(line: &str) -> Option<(usize, usize, &str)> {
    let (numbers, label) = line.strip_prefix('(')?.split_once(") ")?;
    let (index, count) = numbers.split_once('/')?;
    Some((index.trim().parse().ok()?, count.trim().parse().ok()?, label.trim()))
}

/// Splits `linux-cachyos-7.3.6-1-x86_64_v3.pkg.tar.zst` into name and version.
fn package_of(file: &str) -> Option<(&str, &str)> {
    let file = file.split(".pkg").next().unwrap_or(file);
    let (rest, _arch) = file.rsplit_once('-')?;
    let (rest, _release) = rest.rsplit_once('-')?;
    let (name, version) = rest.rsplit_once('-')?;
    let end = file.rfind('-')?;
    let version = &file[name.len() + 1..end.max(name.len() + 1 + version.len())];
    (!name.is_empty()).then_some((name, version))
}

/// Splits `linux-cachyos-7.3.6-1` into name and version.
fn versioned(text: &str) -> Option<(&str, &str)> {
    let (rest, _release) = text.rsplit_once('-')?;
    let (name, _version) = rest.rsplit_once('-')?;
    (!name.is_empty()).then(|| (name, &text[name.len() + 1..]))
}

fn mib(value: &str, unit: &str) -> Option<f32> {
    let value: f32 = value.parse().ok()?;
    Some(match unit {
        "B" => value / 1024.0 / 1024.0,
        "KiB" => value / 1024.0,
        "MiB" => value,
        "GiB" => value * 1024.0,
        "TiB" => value * 1024.0 * 1024.0,
        _ => return None,
    })
}

/// Reads a size such as `442.39 MiB`, in MiB.
pub fn size(text: &str) -> Option<f32> {
    let (value, unit) = text.split_once(' ')?;
    mib(value, unit.trim())
}

/// Parses `11:07` or `1:02:03`.
fn seconds(text: &str) -> Option<u32> {
    text.split(':')
        .try_fold(0u32, |total, part| total.checked_mul(60)?.checked_add(part.parse().ok()?))
}

fn squeeze(text: &str) -> String {
    let indent = text.len() - text.trim_start().len();
    let words: Vec<_> = text.split_whitespace().collect();
    format!("{}{}", &text[..indent], words.join(" "))
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str())
}

/// Reads the packages pacman is about to change.
///
/// `VerbosePkgLists` prints a table, the default is a running list.
fn parse_list(lines: &[String]) -> Vec<Package> {
    let table = lines.iter().rposition(|line| line.starts_with("Package ("));
    if let Some(start) = table {
        return parse_table(&lines[start..]);
    }
    let Some(start) = lines.iter().rposition(|line| line.starts_with("Packages (")) else {
        return Vec::new();
    };
    let mut words = String::new();
    for (index, line) in lines[start..].iter().enumerate() {
        let line = match index {
            0 => line.split_once(") ").map_or("", |(_, rest)| rest),
            _ if line.trim().is_empty() => break,
            _ => line,
        };
        words.push_str(line.trim());
        words.push(' ');
    }
    let mut packages: Vec<Package> = Vec::new();
    for word in words.split_whitespace() {
        if word == "[removal]" {
            if let Some(package) = packages.last_mut() {
                package.from = package.to.take();
            }
        } else if let Some((name, version)) = versioned(word) {
            let (name, to) = (name.to_owned(), Some(version.to_owned()));
            packages.push(Package { name, to, ..Package::default() });
        }
    }
    packages
}

/// Names are aligned to the left of their column and sizes to the right, so a
/// cell belongs to the heading that shares its start or its end.
fn parse_table(lines: &[String]) -> Vec<Package> {
    let headings = cells(&lines[0]);
    let mut packages = Vec::new();
    for line in lines.iter().skip(1) {
        if line.trim().is_empty() {
            if packages.is_empty() {
                continue;
            }
            break;
        }
        if line.starts_with("Total ") {
            break;
        }
        let row = cells(line);
        let Some((_, _, name)) = row.first() else { break };
        let cell = |title: &str| {
            let (start, end, _) = headings.iter().find(|(_, _, heading)| heading == title)?;
            let found = row.iter().skip(1).find(|(s, e, _)| s == start || e == end);
            found.map(|(_, _, text)| text.clone())
        };
        packages.push(Package {
            name: name.rsplit('/').next().unwrap_or(name).to_owned(),
            from: cell("Old Version"),
            to: cell("New Version"),
            size: cell("Download Size").as_deref().and_then(size),
            ..Package::default()
        });
    }
    packages
}

/// Cells of a table row with the columns they start and end at.
fn cells(line: &str) -> Vec<(usize, usize, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut cells = Vec::new();
    let mut start = None;
    for index in 0..=chars.len() {
        let blank = chars.get(index).is_none_or(|c| *c == ' ');
        let gap = blank && chars.get(index + 1).is_none_or(|c| *c == ' ');
        match start {
            None if !blank => start = Some(index),
            Some(from) if gap => {
                cells.push((from, index, chars[from..index].iter().collect()));
                start = None;
            }
            _ => {}
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from pacman 7.1 in a root of its own.
    macro_rules! recording {
        ($name:literal) => {
            include_bytes!(concat!("fixtures/", $name, ".raw")).as_slice()
        };
    }

    fn play(bytes: &[u8]) -> Transaction {
        let mut transaction = Transaction::default();
        transaction.feed(bytes);
        transaction
    }

    /// Plays a recording in small pieces and calls `check` after each. Pieces end
    /// at questions, where the program waited for an answer.
    fn step(bytes: &[u8], mut check: impl FnMut(&Transaction)) -> Transaction {
        const PROMPTS: [&[u8]; 3] = [b"[Y/n] ", b"[y/N] ", b"): "];
        let mut transaction = Transaction::default();
        let mut rest = bytes;
        while !rest.is_empty() {
            let prompt = |end: usize| PROMPTS.iter().any(|prompt| rest[..end].ends_with(prompt));
            let longest = rest.len().min(61);
            let end = (1..=longest).find(|end| prompt(*end)).unwrap_or(longest);
            transaction.feed(&rest[..end]);
            check(&transaction);
            rest = &rest[end..];
        }
        transaction
    }

    #[test]
    fn follows_an_upgrade_from_start_to_end() {
        let mut stages = vec![Stage::Waiting];
        let mut asked = None;
        let mut furthest = 0.0_f32;
        let end = step(recording!("upgrade"), |t| {
            if *stages.last().unwrap() != t.stage {
                stages.push(t.stage);
            }
            if let Some(question) = &t.question {
                asked = Some((question.clone(), t.packages.clone()));
            }
            assert!(t.progress.fraction + 0.001 >= furthest, "progress went back");
            furthest = t.progress.fraction;
        });
        assert_eq!(
            stages,
            [
                Stage::Waiting,
                Stage::Preparing,
                Stage::Downloading,
                Stage::Checking,
                Stage::Installing,
                Stage::Finishing
            ]
        );

        let (question, packages) = asked.unwrap();
        assert!(question.is_proceed() && !question.is_removal());
        assert_eq!(question.text(), "Proceed with installation?");
        assert_eq!(question.answer, Answer::YesNo(true));
        let names: Vec<_> = packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["adwaita-fonts", "aom", "bat", "groff"]);
        assert_eq!(packages[1].from.as_deref(), Some("0.1-1"));
        assert_eq!(packages[1].to.as_deref(), Some("3.15.3-1.1"));
        assert_eq!(packages[1].size, Some(3.04));

        assert_eq!(end.download_size.as_deref(), Some("10.15 MiB"));
        assert_eq!(end.installed_size.as_deref(), Some("30.76 MiB"));
        assert_eq!(end.progress.fraction, 1.0);
        assert_eq!(end.packages.len(), 4);
        assert!(end.packages.iter().all(|p| p.fetched == 1.0 && p.applied == 1.0));
        assert_eq!(end.summary().as_deref(), Some("4 packages updated"));
        let step = end.step.as_ref().unwrap();
        assert_eq!((step.index, step.count), (1, 1));
        assert_eq!(step.label, "Updating linux initcpios...");
        assert_eq!(end.errors(), ["command failed to execute correctly"]);
        assert!(!end.can_stop());
    }

    #[test]
    fn counts_bytes_and_speed_while_downloading() {
        let mut seen = Vec::new();
        step(recording!("upgrade"), |t| {
            if t.stage == Stage::Downloading && t.progress.fraction > 0.3 && seen.is_empty() {
                seen.push((t.progress.clone(), t.packages.clone()));
            }
        });
        let (progress, packages) = seen.pop().unwrap();
        assert!(progress.got > 2.0 && progress.got < 10.0);
        assert!(progress.speed.ends_with("iB/s"));
        assert!(progress.left.is_some());
        assert!(packages.iter().any(|p| p.fetched > 0.0 && p.fetched < 1.0 && p.got > 0.0));
    }

    #[test]
    fn reads_the_short_list() {
        let end = play(recording!("plain"));
        assert_eq!(end.summary().as_deref(), Some("2 packages updated"));
        assert_eq!(end.packages[0].name, "aom");
        assert_eq!(end.packages[0].to.as_deref(), Some("3.15.3-1.1"));
        assert_eq!(end.download_size, None);
        let lines = [
            "Packages (3) linux-cachyos-7.3.6-1  old-tool-1:2.0-1 [removal]".to_owned(),
            "             gtk4-1:4.20.1-1".to_owned(),
            String::new(),
            "Total Installed Size:  1.00 MiB".to_owned(),
        ];
        let packages = parse_list(&lines);
        assert_eq!(packages.len(), 3);
        assert_eq!(packages[1].from.as_deref(), Some("1:2.0-1"));
        assert_eq!(packages[1].to, None);
        assert_eq!(
            (packages[2].name.as_str(), packages[2].to.as_deref()),
            ("gtk4", Some("1:4.20.1-1"))
        );
    }

    #[test]
    fn tables_may_have_empty_cells() {
        let lines: Vec<String> = [
            "Package (3)     Old Version  New Version   Net Change  Download Size",
            "",
            "extra/fresh                  2.0-1           1.00 MiB       0.50 MiB",
            "extra/newer     1.0-1        1.1-1        1024.00 MiB",
            "replaced        3.0-1                       -2.00 MiB",
            "",
            "Total Download Size:   0.50 MiB",
        ]
        .map(str::to_owned)
        .to_vec();
        let packages = parse_list(&lines);
        assert_eq!(packages.len(), 3);
        assert_eq!((packages[0].from.as_deref(), packages[0].to.as_deref()), (None, Some("2.0-1")));
        assert_eq!(packages[0].size, Some(0.5));
        assert_eq!(packages[1].size, None);
        assert_eq!((packages[2].from.as_deref(), packages[2].to.as_deref()), (Some("3.0-1"), None));
    }

    #[test]
    fn follows_a_removal() {
        let mut asked = false;
        let end = step(recording!("remove"), |t| {
            asked |= t.question.as_ref().is_some_and(Question::is_removal);
        });
        assert!(asked);
        assert_eq!(end.summary().as_deref(), Some("bat removed"));
        assert_eq!(end.removed_size.as_deref(), Some("4.98 MiB"));
        assert_eq!(end.packages[0].from.as_deref(), Some("0.26.1-3.1"));
        assert_eq!(end.packages[0].to, None);
    }

    #[test]
    fn lists_the_choices_of_a_question() {
        let mut questions: Vec<Question> = Vec::new();
        step(recording!("provider"), |t| {
            if let Some(question) = &t.question
                && !questions.contains(question)
            {
                questions.push(question.clone());
            }
        });
        assert_eq!(questions.len(), 2);
        let Answer::Number(choices, default) = &questions[0].answer else { panic!() };
        assert_eq!(default, "1");
        assert_eq!(choices.len(), 2);
        assert_eq!((choices[1].name.as_str(), choices[1].origin.as_str()), ("ncurses", "core"));
        let Answer::Number(choices, _) = &questions[1].answer else { panic!() };
        assert_eq!(choices[0].name, "libverto");
        assert!(questions[1].context[0].contains("libverto-module-base"));
    }

    #[test]
    fn knows_every_kind_of_question() {
        let ask = |text: &str| play(text.as_bytes()).question;
        let replace =
            ask(":: Replace pipewire-media-session with extra/wireplumber? [Y/n] ").unwrap();
        assert_eq!(replace.text(), "Replace pipewire-media-session with extra/wireplumber?");
        assert!(!replace.is_proceed());
        let conflict =
            ask(":: jack2 and pipewire-jack are in conflict. Remove jack2? [y/N] ").unwrap();
        assert_eq!(conflict.answer, Answer::YesNo(false));
        let skip = ask(
            ":: The following package cannot be upgraded due to unresolvable dependencies:\r\n      \
             foo\r\n\r\n:: Do you want to skip the above package for this upgrade? [y/N] ",
        )
        .unwrap();
        let lead = ":: The following package cannot be upgraded due to unresolvable dependencies:";
        assert_eq!(skip.context, [lead, "      foo"]);
        assert_eq!(ask("Enter a selection (default=all): ").unwrap().answer, Answer::Text);
        // An answer that has been given is not a question any more.
        assert_eq!(ask(":: Proceed with installation? [Y/n] y"), None);
        assert_eq!(ask(":: Proceed with installation? [Y/n] \r\n"), None);
        assert_eq!(ask("resolving dependencies..."), None);
    }

    #[test]
    fn suspects_a_question_only_when_asked_to() {
        let mut transaction = play(b"Overwrite the old image? ");
        assert_eq!(transaction.question, None);
        transaction.suspect();
        assert_eq!(transaction.question.unwrap().answer, Answer::Text);
        for output in ["still working", "(1/4) upgrading bat  [----]  40%"] {
            let mut transaction = play(output.as_bytes());
            transaction.suspect();
            assert_eq!(transaction.question, None, "{output}");
        }
    }

    #[test]
    fn explains_failures() {
        let locked = play(recording!("locked")).failure(Some(1));
        assert_eq!(locked.title, "Another package manager is running");
        let missing = play(recording!("missing")).failure(Some(1));
        assert_eq!(missing.title, "no-such-package-here was not found");
        let blocked = play(recording!("blocked")).failure(Some(1));
        assert_eq!(
            blocked.hint,
            "removing bat breaks dependency 'bat' required by cachyos-fish-config"
        );
        let unreachable = play(recording!("unreachable"));
        assert!(unreachable.packages.iter().all(|package| package.failed));
        let unreachable = unreachable.failure(Some(1));
        assert_eq!(unreachable.title, "figlet could not be downloaded");
        assert!(unreachable.mirrors && unreachable.hint.starts_with("127.0.0.1:9"));
        let denied = play(b"Error executing command as another user: Not authorized\r\n");
        assert_eq!(denied.failure(Some(127)).title, "No permission");
        assert_eq!(play(b"error: something new\r\n").failure(Some(1)).title, "Something new");
        assert!(play(b"").failure(Some(9)).hint.contains("code 9"));
    }

    #[test]
    fn notices_when_there_is_nothing_to_do() {
        assert!(play(recording!("nothing")).has_nothing_to_do());
        assert!(!play(recording!("decline")).has_nothing_to_do());
    }

    #[test]
    fn stopping_a_download_changes_nothing() {
        let end = step(recording!("stop"), |_| {});
        assert_eq!(end.stage, Stage::Downloading);
        assert!(end.can_stop() && end.changes.is_empty());
        // A single download has no total of its own.
        assert!(end.progress.fraction > 0.0);
    }

    #[test]
    fn a_second_transaction_starts_over() {
        let mut transaction = play(recording!("plain"));
        assert_eq!(transaction.stage, Stage::Finishing);
        transaction.feed(b"resolving dependencies...\r\n");
        assert_eq!(transaction.stage, Stage::Preparing);
        transaction.feed(b"Packages (1) tree-2.2-1\r\n\r\n:: Proceed with installation? [Y/n] ");
        assert_eq!(transaction.packages.len(), 1);
        transaction
            .feed(b"y\r\n:: Processing package changes...\r\n(1/1) installing tree [##] 100%\r\n");
        assert_eq!(transaction.summary().as_deref(), Some("2 packages updated, 1 installed"));
    }

    #[test]
    fn output_keeps_the_words_and_drops_the_bars() {
        let output = play(recording!("plain")).output(100);
        assert!(output.contains("(2/2) upgrading bat  100%"));
        assert!(output.contains(":: Proceed with installation? [Y/n] y"));
        assert!(!output.contains("####"));
        assert_eq!(play(recording!("plain")).output(2).lines().count(), 1);
    }

    #[test]
    fn reads_file_names() {
        let parse = |file| package_of(file).unwrap();
        assert_eq!(parse("linux-cachyos-7.3.6-1-x86_64_v3"), ("linux-cachyos", "7.3.6-1"));
        assert_eq!(parse("gtk4-1:4.20.1-1-x86_64.pkg.tar.zst"), ("gtk4", "1:4.20.1-1"));
        assert_eq!(package_of("core"), None);
        assert_eq!(seconds("11:07"), Some(667));
        assert_eq!(seconds("1:00:01"), Some(3601));
        assert_eq!(seconds("--:--"), None);
        assert_eq!(size("1.00 GiB"), Some(1024.0));
    }
}
