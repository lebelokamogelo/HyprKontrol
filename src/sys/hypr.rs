//! Hyprland: live changes through `hyprctl eval`, and their persistence.
//!
//! The user's Lua modules are never rewritten. Overrides live in a generated
//! module, `config/hyprkontrol.lua`, which is required last so that it wins.
//! The only edit to a hand-written file is that one `require` line.
//!
//! The generated module is an output. Its source is [`Store`], kept as JSON
//! beside the program's own configuration, because reading our Lua back would
//! mean embedding a Lua parser to recover what we already knew.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::{Cmd, Error, Result, config_dir, edit};

const MODULE: &str = "hyprkontrol";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    /// Colours are `rgba(rrggbbaa)`, as in Hyprland's own configuration.
    Gradient {
        colors: Vec<String>,
        angle: i64,
    },
}

impl Value {
    /// A float at the precision Hyprland options are meant to be set with.
    ///
    /// Sliders work in single precision and count up from the start of their
    /// range, so a handle at zero reports something like 1.5e-8. Kept as is, that
    /// would be saved, and would differ from the zero it stands for.
    pub fn float(value: f32) -> Self {
        Value::Float((f64::from(value) * 1000.0).round() / 1000.0 + 0.0)
    }

    pub fn bool(&self) -> bool {
        matches!(self, Value::Bool(true)) || matches!(self, Value::Int(n) if *n != 0)
    }

    pub fn number(&self) -> f32 {
        match self {
            Value::Int(n) => *n as f32,
            Value::Float(n) => *n as f32,
            _ => 0.0,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Value::Text(text) => text,
            _ => "",
        }
    }

    fn lua(&self) -> String {
        match self {
            Value::Bool(value) => value.to_string(),
            Value::Int(value) => value.to_string(),
            // `{:?}` keeps the decimal point, so Lua sees a float and not an integer.
            Value::Float(value) => format!("{:?}", (value * 1000.0).round() / 1000.0),
            Value::Text(text) => quote(text),
            Value::Gradient { colors, angle } => {
                let colors: Vec<_> = colors.iter().map(|color| quote(color)).collect();
                format!("{{ colors = {{ {} }}, angle = {angle} }}", colors.join(", "))
            }
        }
    }

    /// Parses one `hyprctl -j getoption` reply.
    fn from_reply(reply: &Json) -> Option<Self> {
        if let Some(value) = reply.get("bool").and_then(Json::as_bool) {
            return Some(Value::Bool(value));
        }
        if let Some(value) = reply.get("int").and_then(Json::as_i64) {
            return Some(Value::Int(value));
        }
        if let Some(value) = reply.get("float").and_then(Json::as_f64) {
            return Some(Value::Float(value));
        }
        if let Some(value) = reply.get("str").and_then(Json::as_str) {
            // Hyprland prints unset strings as "[[EMPTY]]".
            let value = if value == "[[EMPTY]]" { "" } else { value };
            return Some(Value::Text(value.to_owned()));
        }
        // Gaps are reported per side. The panel edits them as one number.
        if let Some(value) = reply.get("css").and_then(Json::as_str) {
            return value.split_whitespace().next()?.parse().ok().map(Value::Int);
        }
        reply.get("gradient").and_then(Json::as_str).map(parse_gradient)
    }
}

/// Converts `ff82dccc ff007d6f 45deg`, with alpha first, to a gradient.
fn parse_gradient(text: &str) -> Value {
    let mut colors = Vec::new();
    let mut angle = 0;
    for part in text.split_whitespace() {
        if let Some(degrees) = part.strip_suffix("deg") {
            angle = degrees.parse().unwrap_or(0);
        } else if part.len() == 8 {
            colors.push(format!("rgba({}{})", &part[2..], &part[..2]));
        }
    }
    Value::Gradient { colors, angle }
}

/// Quotes a string for Lua. Control characters are escaped numerically, so the
/// result is always a single line.
fn quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            '"' | '\\' => quoted.extend(['\\', c]),
            c if c.is_control() => {
                let _ = write!(quoted, "\\{:03}", c as u32);
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub enabled: bool,
    pub speed: f32,
    pub bezier: String,
    #[serde(default)]
    pub style: String,
}

impl Animation {
    fn lua(&self, leaf: &str) -> String {
        let mut spec = format!(
            "hl.animation({{ leaf = {}, enabled = {}, speed = {}",
            quote(leaf),
            self.enabled,
            Value::Float(self.speed.into()).lua()
        );
        // "easy" and "rubber" in the stock configuration are springs, which are
        // a separate field from bezier curves.
        let curve = if SPRINGS.contains(&self.bezier.as_str()) { "spring" } else { "bezier" };
        if !self.bezier.is_empty() {
            let _ = write!(spec, ", {curve} = {}", quote(&self.bezier));
        }
        if !self.style.is_empty() {
            let _ = write!(spec, ", style = {}", quote(&self.style));
        }
        spec + " })"
    }
}

pub const SPRINGS: [&str; 2] = ["easy", "rubber"];

/// A monitor rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub output: String,
    pub mode: String,
    pub position: String,
    pub scale: f64,
    #[serde(default)]
    pub transform: u8,
    #[serde(default)]
    pub mirror: String,
    #[serde(default)]
    pub disabled: bool,
}

impl Rule {
    pub fn lua(&self) -> String {
        if self.disabled {
            return format!("hl.monitor({{ output = {}, disabled = true }})", quote(&self.output));
        }
        let mut rule = format!(
            "hl.monitor({{ output = {}, mode = {}, position = {}, scale = {}, transform = {}",
            quote(&self.output),
            quote(&self.mode),
            quote(&self.position),
            Value::Float(self.scale).lua(),
            self.transform
        );
        if !self.mirror.is_empty() {
            let _ = write!(rule, ", mirror = {}", quote(&self.mirror));
        }
        rule + " })"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bind {
    /// Keys as Hyprland spells them, for example `SUPER + F9`.
    pub keys: String,
    pub command: String,
}

impl Bind {
    fn lua(&self) -> String {
        format!("hl.bind({}, hl.dsp.exec_cmd({}))", quote(&self.keys), quote(&self.command))
    }
}

/// A single change. `Reset` variants restore a value that came from the user's
/// own configuration, so the override is dropped instead of pinned.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Set(String, Value),
    Reset(String, Value),
    Animate(String, Animation),
    ResetAnimation(String, Animation),
    Monitor(Rule),
    Bind(Bind),
    Unbind(Bind),
}

impl Edit {
    /// The statement that takes effect in the running compositor.
    fn lua(&self) -> String {
        match self {
            Edit::Set(key, value) | Edit::Reset(key, value) => {
                format!("hl.config({})", nest(&[(key.clone(), value.clone())]))
            }
            Edit::Animate(leaf, spec) | Edit::ResetAnimation(leaf, spec) => spec.lua(leaf),
            Edit::Monitor(rule) => rule.lua(),
            Edit::Bind(bind) => bind.lua(),
            Edit::Unbind(bind) => format!("hl.unbind({})", quote(&bind.keys)),
        }
    }

    fn record(self, store: &mut Store) {
        match self {
            Edit::Set(key, value) => drop(store.options.insert(key, value)),
            Edit::Reset(key, _) => drop(store.options.remove(&key)),
            Edit::Animate(leaf, spec) => drop(store.animations.insert(leaf, spec)),
            Edit::ResetAnimation(leaf, _) => drop(store.animations.remove(&leaf)),
            Edit::Monitor(rule) => drop(store.monitors.insert(rule.output.clone(), rule)),
            Edit::Bind(bind) => {
                store.binds.retain(|known| known.keys != bind.keys);
                store.binds.push(bind);
            }
            Edit::Unbind(bind) => store.binds.retain(|known| known.keys != bind.keys),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Edit::Set(key, value) | Edit::Reset(key, value) => format!("{key} = {}", value.lua()),
            other => other.lua(),
        }
    }
}

/// Everything this program overrides in Hyprland.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub options: BTreeMap<String, Value>,
    pub animations: BTreeMap<String, Animation>,
    pub monitors: BTreeMap<String, Rule>,
    pub binds: Vec<Bind>,
}

impl Store {
    pub fn load() -> Self {
        std::fs::read_to_string(store_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Builds the edits for changing `key`, with the right kind of inverse.
    pub fn set(&self, key: &str, before: Value, after: Value) -> super::Change {
        let key = key.to_owned();
        let undo = if self.options.contains_key(&key) {
            Edit::Set(key.clone(), before)
        } else {
            Edit::Reset(key.clone(), before)
        };
        super::Change::new(super::Op::Hypr(vec![Edit::Set(key, after)]))
            .undo(super::Op::Hypr(vec![undo]))
    }

    fn lua(&self) -> String {
        let mut text = String::from(
            "-- Generated by HyprKontrol. Changes made here are overwritten.\n\
             -- Remove the require line in hyprland.lua to turn these overrides off.\n",
        );
        if !self.options.is_empty() {
            let options: Vec<_> =
                self.options.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
            let _ = write!(text, "\nhl.config({})\n", nest(&options));
        }
        for (leaf, spec) in &self.animations {
            let _ = write!(text, "\n{}", spec.lua(leaf));
        }
        for rule in self.monitors.values() {
            let _ = write!(text, "\n{}", rule.lua());
        }
        for bind in &self.binds {
            let _ = write!(text, "\n{}", bind.lua());
        }
        text.trim_end().to_owned() + "\n"
    }

    fn save(&self) -> Result<()> {
        edit::write_atomic(&store_path(), &serde_json::to_string_pretty(self)?)?;
        edit::write_atomic(&dir().join(format!("config/{MODULE}.lua")), &self.lua())
    }
}

/// Turns dotted keys into a nested Lua table.
fn nest(options: &[(String, Value)]) -> String {
    enum Node<'a> {
        Leaf(&'a Value),
        Table(BTreeMap<&'a str, Node<'a>>),
    }

    fn render(node: &Node<'_>) -> String {
        match node {
            Node::Leaf(value) => value.lua(),
            Node::Table(entries) => {
                let body: Vec<_> =
                    entries.iter().map(|(key, node)| format!("{key} = {}", render(node))).collect();
                format!("{{ {} }}", body.join(", "))
            }
        }
    }

    let mut root = BTreeMap::new();
    for (key, value) in options {
        let mut table = &mut root;
        let mut parts = key.split('.').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                table.insert(part, Node::Leaf(value));
                break;
            }
            let node = table.entry(part).or_insert_with(|| Node::Table(BTreeMap::new()));
            match node {
                Node::Table(inner) => table = inner,
                // A key cannot be both a value and a table, so the first one wins.
                Node::Leaf(_) => break,
            }
        }
    }
    render(&Node::Table(root))
}

fn dir() -> PathBuf {
    config_dir().join("hypr")
}

fn store_path() -> PathBuf {
    config_dir().join("hyprkontrol/hyprland.json")
}

fn entry_point() -> PathBuf {
    dir().join("hyprland.lua")
}

/// Whether changes can outlive the session. Hyprland also reads the older
/// `hyprland.conf`, which has no equivalent of `eval` and is not supported.
pub fn persistent() -> bool {
    entry_point().is_file()
}

/// Evaluates Lua in the running compositor.
pub async fn eval(lua: &str) -> Result<()> {
    let run = async |lua: &str| Cmd::new("hyprctl").args(["eval", lua]).output().await;
    let reason = match run(lua).await {
        Ok(reply) if reply.trim() == "ok" => return Ok(()),
        Ok(reply) => reply.trim().to_owned(),
        Err(error) => error.to_string(),
    };
    // Hyprland keeps an error bar on screen until something evaluates cleanly.
    // The reason is shown in the panel, so the bar has nothing to add.
    let _ = run("return 1").await;
    Err(Error::new(explain(&reason)))
}

/// Strips the position of a Lua error. It points into code the user never wrote.
fn explain(reason: &str) -> &str {
    reason.rsplit(":1: ").next().unwrap_or(reason).trim_start_matches("error: ")
}

/// Shows a value on the desktop without saving it, used while a slider moves.
pub async fn preview(key: String, value: Value) -> Result<()> {
    eval(&Edit::Set(key, value).lua()).await
}

/// Applies edits live, then records them.
///
/// The order matters: an edit Hyprland refuses must never reach the generated
/// module, where it would break the configuration at the next login.
pub async fn apply(edits: Vec<Edit>) -> Result<()> {
    let statements: Vec<_> = edits.iter().map(Edit::lua).collect();
    eval(&statements.join(" ")).await?;
    if !persistent() {
        return Ok(());
    }
    let mut store = Store::load();
    for edit in edits {
        edit.record(&mut store);
    }
    store.save()?;
    ensure_required()
}

fn ensure_required() -> Result<()> {
    let path = entry_point();
    let text = std::fs::read_to_string(&path)?;
    let Some(updated) = with_require(&text) else { return Ok(()) };
    edit::backup_once(&path)?;
    edit::write_atomic(&path, &updated)
}

/// Appends the `require` line, or returns `None` when it is already there.
fn with_require(text: &str) -> Option<String> {
    let line = format!("require(\"config.{MODULE}\")");
    let present = text.lines().any(|l| l.split("--").next().unwrap_or("").contains(&line));
    (!present).then(|| format!("{}\n\n-- Overrides from HyprKontrol\n{line}\n", text.trim_end()))
}

/// Reads current option values. Keys use dots, as in Lua.
pub async fn options(keys: &'static [&'static str]) -> HashMap<String, Value> {
    let batch: Vec<_> =
        keys.iter().map(|key| format!("getoption {}", key.replace('.', ":"))).collect();
    let Ok(reply) = Cmd::new("hyprctl").args(["-j", "--batch", &batch.join(";")]).output().await
    else {
        return HashMap::new();
    };
    parse_options(&reply)
}

/// A batch reply is a series of JSON documents, not an array.
fn parse_options(reply: &str) -> HashMap<String, Value> {
    serde_json::Deserializer::from_str(reply)
        .into_iter::<Json>()
        .map_while(std::result::Result::ok)
        .filter_map(|document| {
            let key = document.get("option")?.as_str()?.replace(':', ".");
            Some((key, Value::from_reply(&document)?))
        })
        .collect()
}

pub async fn json(command: &str) -> Result<Json> {
    let reply = Cmd::new("hyprctl").args(["-j", command]).output().await?;
    Ok(serde_json::from_str(&reply)?)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Animated {
    pub leaf: String,
    pub overridden: bool,
    pub spec: Animation,
}

/// Returns the animation leaves and the names of the available curves.
pub async fn animations() -> Result<(Vec<Animated>, Vec<String>)> {
    let reply = json("animations").await?;
    let text = |value: &Json, key: &str| {
        value.get(key).and_then(Json::as_str).unwrap_or_default().to_owned()
    };
    let list =
        |index: usize| reply.get(index).and_then(Json::as_array).cloned().unwrap_or_default();
    let animations = list(0)
        .iter()
        .map(|entry| Animated {
            leaf: text(entry, "name"),
            overridden: entry.get("overridden").and_then(Json::as_bool).unwrap_or(false),
            spec: Animation {
                enabled: entry.get("enabled").and_then(Json::as_bool).unwrap_or(true),
                speed: entry.get("speed").and_then(Json::as_f64).unwrap_or(1.0) as f32,
                bezier: text(entry, "bezier"),
                style: text(entry, "style"),
            },
        })
        .collect();
    let mut curves: Vec<_> = list(1).iter().map(|entry| text(entry, "name")).collect();
    curves.extend(SPRINGS.map(str::to_owned));
    curves.sort();
    curves.dedup();
    Ok((animations, curves))
}

/// A shortcut read from the user's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub keys: String,
    pub action: String,
}

/// Reads shortcuts from `config/binds.lua`.
///
/// `hyprctl binds` lists Lua dispatchers as opaque references, so it cannot say
/// what a shortcut does. The source can. This understands the shapes the stock
/// configuration uses: string literals, `..` concatenation and string variables.
pub fn shortcuts() -> Vec<Shortcut> {
    std::fs::read_to_string(dir().join("config/binds.lua"))
        .map(|text| parse_shortcuts(&text))
        .unwrap_or_default()
}

fn parse_shortcuts(text: &str) -> Vec<Shortcut> {
    let mut variables: HashMap<&str, String> = HashMap::new();
    let mut shortcuts = Vec::new();
    for line in text.lines() {
        let line = strip_comment(line).trim();
        if let Some((name, value)) = line.strip_prefix("local ").and_then(|l| l.split_once('=')) {
            let value = value.trim();
            if let Some(value) = evaluate(value, &variables) {
                variables.insert(name.trim(), value);
            }
        } else if let Some(body) = line.strip_prefix("hl.bind(") {
            let Some((keys, action)) = split_argument(body) else { continue };
            let action = action.trim().trim_start_matches("hl.dsp.");
            shortcuts.push(Shortcut {
                // Shortcuts bound in a loop have a key that only exists at runtime.
                keys: evaluate(keys, &variables).unwrap_or_else(|| {
                    evaluate(keys.rsplit_once("..").map_or(keys, |k| k.0), &variables)
                        .map_or_else(|| keys.to_owned(), |prefix| format!("{prefix}<number>"))
                }),
                action: describe_action(action, &variables),
            });
        }
    }
    shortcuts
}

/// Cuts a trailing Lua comment. `--` is common inside strings here, since
/// launcher prefixes such as `"uwsm-app -- "` end in it.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut previous = ' ';
    for (index, c) in line.char_indices() {
        match c {
            '"' if previous != '\\' => quoted = !quoted,
            '-' if !quoted && previous == '-' => return &line[..index - 1],
            _ => {}
        }
        previous = c;
    }
    line
}

/// Splits at the first comma that is outside strings and brackets.
fn split_argument(text: &str) -> Option<(&str, &str)> {
    let (mut depth, mut quoted) = (0i32, false);
    for (index, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' | '{' | '[' if !quoted => depth += 1,
            ')' | '}' | ']' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => return Some((&text[..index], &text[index + 1..])),
            _ => {}
        }
    }
    None
}

/// Resolves a concatenation of string literals and known variables.
fn evaluate(expression: &str, variables: &HashMap<&str, String>) -> Option<String> {
    expression
        .split("..")
        .map(str::trim)
        .map(|part| {
            let literal = part
                .strip_prefix('"')
                .and_then(|p| p.strip_suffix('"'))
                .or_else(|| part.strip_prefix("[[").and_then(|p| p.strip_suffix("]]")));
            literal.map(str::to_owned).or_else(|| variables.get(part).cloned())
        })
        .collect()
}

/// Phrases a dispatcher call for display.
fn describe_action(action: &str, variables: &HashMap<&str, String>) -> String {
    if action.starts_with("function") {
        return "Custom function".to_owned();
    }
    let Some((name, rest)) = action.split_once('(') else {
        return action.trim_end_matches(')').to_owned();
    };
    let mut depth = 1;
    let end = rest.find(|c| {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        depth == 0
    });
    let argument = &rest[..end.unwrap_or(rest.len())];
    let argument = evaluate(argument, variables).unwrap_or_else(|| {
        let words: Vec<_> = argument
            .split(|c: char| "{}=\",".contains(c) || c.is_whitespace())
            .filter(|word| !word.is_empty())
            .collect();
        words.join(" ")
    });
    match name {
        "exec_cmd" => argument,
        _ => {
            let mut phrase = format!("{} {argument}", name.replace(['.', '_'], " "));
            if let Some(first) = phrase.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            phrase.trim().to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_dotted_keys() {
        let options = [
            ("decoration.blur.size".to_owned(), Value::Int(4)),
            ("decoration.rounding".to_owned(), Value::Int(10)),
            ("general.layout".to_owned(), Value::Text("dwindle".into())),
        ];
        assert_eq!(
            nest(&options),
            r#"{ decoration = { blur = { size = 4 }, rounding = 10 }, general = { layout = "dwindle" } }"#
        );
    }

    #[test]
    fn lua_strings_cannot_break_out() {
        assert_eq!(quote("a\"b\\c\nd"), "\"a\\\"b\\\\c\\010d\"");
        let bind = Bind { keys: "SUPER + X".into(), command: "x\"); os.execute(\"y".into() };
        assert_eq!(bind.lua(), r#"hl.bind("SUPER + X", hl.dsp.exec_cmd("x\"); os.execute(\"y"))"#);
    }

    #[test]
    fn slider_noise_is_rounded_away() {
        assert_eq!(Value::float(1.490_116_1e-8), Value::Float(0.0));
        assert_eq!(Value::float(-1.490_116_1e-8).lua(), "0.0");
        assert_eq!(Value::float(0.85), Value::Float(0.85));
    }

    #[test]
    fn floats_stay_floats() {
        assert_eq!(Value::Float(1.0).lua(), "1.0");
        assert_eq!(Value::Float(0.850_000_02).lua(), "0.85");
    }

    #[test]
    fn parses_batched_options() {
        let reply = "{\"option\": \"general:gaps_in\", \"css\": \"3 3 3 3\", \"set\": true }\n\n\n\
                     {\"option\": \"general:col.active_border\", \"gradient\": \"ff82dccc ff007d6f 45deg\", \"set\": true }\n\n\n\
                     {\"option\": \"input:kb_variant\", \"str\": \"[[EMPTY]]\", \"set\": false }";
        let options = parse_options(reply);
        assert_eq!(options["general.gaps_in"], Value::Int(3));
        assert_eq!(options["input.kb_variant"], Value::Text(String::new()));
        assert_eq!(
            options["general.col.active_border"],
            Value::Gradient {
                colors: vec!["rgba(82dcccff)".into(), "rgba(007d6fff)".into()],
                angle: 45
            }
        );
    }

    #[test]
    fn errors_are_reduced_to_their_reason() {
        let reply = "error: return hl.config({ a = 1 });:1: unknown config key 'a'";
        assert_eq!(explain(reply), "unknown config key 'a'");
        assert_eq!(explain("hyprctl is not installed"), "hyprctl is not installed");
    }

    #[test]
    fn require_is_added_once_and_ignores_comments() {
        let text = "require(\"config.binds\")\n-- require(\"config.hyprkontrol\")\n";
        let once = with_require(text).unwrap();
        assert!(once.ends_with("require(\"config.hyprkontrol\")\n"));
        assert_eq!(with_require(&once), None);
    }

    #[test]
    fn undo_of_a_first_override_drops_it() {
        let change = Store::default().set("decoration.rounding", Value::Int(10), Value::Int(4));
        let Some(super::super::Op::Hypr(edits)) = change.undo else { panic!("expected an undo") };
        assert!(
            matches!(&edits[0], Edit::Reset(key, Value::Int(10)) if key == "decoration.rounding")
        );

        let mut store = Store::default();
        Edit::Set("decoration.rounding".into(), Value::Int(4)).record(&mut store);
        assert_eq!(store.options.len(), 1);
        edits[0].clone().record(&mut store);
        assert!(store.options.is_empty());
    }

    /// Saves the value that is already set, into a throwaway configuration:
    ///
    ///     XDG_CONFIG_HOME=$(mktemp -d) cargo test saves_to_a_sandbox -- --ignored
    #[tokio::test]
    #[ignore = "needs a running Hyprland"]
    async fn saves_to_a_sandbox() {
        let sandbox = config_dir();
        assert!(sandbox.starts_with(std::env::temp_dir()), "refusing to touch {sandbox:?}");
        std::fs::create_dir_all(dir().join("config")).unwrap();
        std::fs::write(entry_point(), "require(\"config.binds\")\n").unwrap();

        let current = options(&["decoration.rounding"]).await;
        let value = current["decoration.rounding"].clone();
        let change = Store::load().set("decoration.rounding", value.clone(), value.clone());
        change.apply.apply().await.unwrap();

        assert_eq!(Store::load().options["decoration.rounding"], value);
        let module = std::fs::read_to_string(dir().join("config/hyprkontrol.lua")).unwrap();
        assert!(module.contains("rounding"), "{module}");
        let entry = std::fs::read_to_string(entry_point()).unwrap();
        assert!(entry.ends_with("require(\"config.hyprkontrol\")\n"), "{entry}");
        assert!(dir().join("hyprland.lua.before-hyprkontrol").exists());

        // The inverse of a first override removes it again.
        change.undo.unwrap().apply().await.unwrap();
        assert!(Store::load().options.is_empty());

        let refused =
            apply(vec![Edit::Set("decoration.no_such_option".into(), Value::Int(1))]).await;
        let refused = refused.expect_err("Hyprland should refuse an unknown option");
        assert_eq!(refused.to_string(), "unknown config key 'decoration.no_such_option'");
        let errors = Cmd::new("hyprctl").arg("configerrors").output().await.unwrap();
        assert!(errors.trim().is_empty(), "the error bar must not linger: {errors}");
        assert!(Store::load().options.is_empty(), "a refused edit must not be saved");
    }

    #[test]
    fn reads_shortcuts_from_lua() {
        let text = r#"
local mainMod = "SUPER"
local launchPrefix = "uwsm-app -- " -- leave empty without UWSM
hl.bind(mainMod .. " + Return", hl.dsp.exec_cmd(launchPrefix .. "kitty"))
hl.bind(mainMod .. " + J",           hl.dsp.layout("togglesplit"))
hl.bind(mainMod .. " + Left",  hl.dsp.focus({ direction = "left" }))
    local key = i % 10
    hl.bind(mainMod .. " + CTRL + " .. key, hl.dsp.window.move({ workspace = i }))
hl.bind("XF86AudioMute", hl.dsp.exec_cmd("wpctl set-mute @DEFAULT@ toggle"), { locked = true })
"#;
        let shortcuts = parse_shortcuts(text);
        assert_eq!(shortcuts[0].keys, "SUPER + Return");
        assert_eq!(shortcuts[0].action, "uwsm-app -- kitty");
        assert_eq!(shortcuts[1].action, "Layout togglesplit");
        assert_eq!(shortcuts[2].action, "Focus direction left");
        assert_eq!(shortcuts[3].keys, "SUPER + CTRL + <number>");
        assert_eq!(shortcuts[4].action, "wpctl set-mute @DEFAULT@ toggle");
    }
}
