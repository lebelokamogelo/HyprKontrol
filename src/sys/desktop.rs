//! Desktop look and behaviour outside the compositor: GTK settings, wallpaper,
//! default applications, startup entries and notifications.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{Change, Cmd, Error, Op, Result, config_dir, edit, home, query};

const INTERFACE: &str = "org.gnome.desktop.interface";

/// Look of GTK applications.
///
/// GSettings is the source of truth. The desktop portal passes the colour scheme
/// on to everything else, while Qt keeps its own theme in qt6ct.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub accent: String,
    pub gtk_theme: String,
    pub icon_theme: String,
    pub cursor_theme: String,
    pub cursor_size: u32,
    pub font: String,
    pub mono_font: String,
    pub text_scale: f32,
}

pub const ACCENTS: [(&str, u32); 9] = [
    ("blue", 0x3584e4),
    ("teal", 0x2190a4),
    ("green", 0x3a944a),
    ("yellow", 0xc88800),
    ("orange", 0xed5b00),
    ("red", 0xe62d42),
    ("pink", 0xd56199),
    ("purple", 0x9141ac),
    ("slate", 0x6f8396),
];

pub async fn look() -> Look {
    let text = query("gsettings", ["list-recursively", INTERFACE]).await.unwrap_or_default();
    parse_look(&text)
}

fn parse_look(text: &str) -> Look {
    let values: BTreeMap<&str, &str> = text
        .lines()
        .filter_map(|line| line.strip_prefix(INTERFACE)?.trim().split_once(' '))
        .map(|(key, value)| (key, value.trim().trim_matches('\'')))
        .collect();
    let get = |key: &str| (*values.get(key).unwrap_or(&"")).to_owned();
    Look {
        dark: get("color-scheme") == "prefer-dark",
        accent: get("accent-color"),
        gtk_theme: get("gtk-theme"),
        icon_theme: get("icon-theme"),
        cursor_theme: get("cursor-theme"),
        cursor_size: get("cursor-size").parse().unwrap_or(24),
        font: get("font-name"),
        mono_font: get("monospace-font-name"),
        text_scale: get("text-scaling-factor").parse().unwrap_or(1.0),
    }
}

/// Family of a Pango font description such as `Adwaita Sans 11`.
pub fn font_family(description: &str) -> &str {
    match description.rsplit_once(' ') {
        Some((family, size)) if size.parse::<f32>().is_ok() => family,
        _ => description,
    }
}

fn gset(key: &str, value: &str) -> Cmd {
    Cmd::new("gsettings").args(["set", INTERFACE, key, value])
}

pub fn set(key: &str, before: &str, after: &str) -> Change {
    Change::new(gset(key, after)).undo(gset(key, before))
}

/// Switches between dark and light.
///
/// The colour scheme alone is not enough for GTK 3, which only looks at the
/// theme name. Themes that come as a pair (`adw-gtk3`, `adw-gtk3-dark`) are
/// switched along with it.
pub fn set_dark(look: &Look, dark: bool) -> Change {
    let commands = |dark: bool, theme: &str| {
        let scheme = if dark { "prefer-dark" } else { "prefer-light" };
        let mut commands = vec![gset("color-scheme", scheme)];
        let bare = theme.trim_end_matches("-dark");
        let paired = if dark { format!("{bare}-dark") } else { bare.to_owned() };
        if paired != look.gtk_theme && themes().contains(&paired) {
            commands.push(gset("gtk-theme", &paired));
        }
        commands
    };
    Change::new(Op::Run(commands(dark, &look.gtk_theme))).undo(Op::Run(vec![
        gset("color-scheme", if look.dark { "prefer-dark" } else { "prefer-light" }),
        gset("gtk-theme", &look.gtk_theme),
    ]))
}

/// Cursor changes have to reach both GTK and the compositor.
pub fn set_cursor(before: (&str, u32), after: (&str, u32)) -> Change {
    let commands = |(theme, size): (&str, u32)| {
        let size = size.to_string();
        Op::Run(vec![
            gset("cursor-theme", theme),
            gset("cursor-size", &size),
            Cmd::new("hyprctl").args(["setcursor", theme, &size]),
        ])
    };
    Change::new(commands(after)).undo(commands(before))
}

fn data_dirs(kind: &str) -> Vec<PathBuf> {
    vec![
        home().join(format!(".{kind}")),
        home().join(format!(".local/share/{kind}")),
        PathBuf::from(format!("/usr/share/{kind}")),
    ]
}

fn subdirs(kind: &str, accept: impl Fn(&Path) -> bool) -> Vec<String> {
    let mut names: Vec<String> = data_dirs(kind)
        .iter()
        .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| accept(path))
        .filter_map(|path| Some(path.file_name()?.to_str()?.to_owned()))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

pub fn themes() -> Vec<String> {
    subdirs("themes", |path| path.join("gtk-3.0").is_dir() || path.join("gtk-4.0").is_dir())
}

/// Icon themes list their directories. Cursor themes share the same folder and
/// do not, which is the only reliable difference.
pub fn icon_themes() -> Vec<String> {
    subdirs("icons", |path| {
        let hidden = ["hicolor", "default", "locolor"];
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        !hidden.contains(&name)
            && super::read(path.join("index.theme"))
                .is_some_and(|text| text.lines().any(|line| line.starts_with("Directories=")))
    })
}

pub fn cursor_themes() -> Vec<String> {
    subdirs("icons", |path| path.join("cursors").is_dir() || path.join("hyprcursors").is_dir())
}

/// Installed font families, as fontconfig names them.
pub async fn fonts() -> (Vec<String>, Vec<String>) {
    let parse = |text: Option<String>| {
        let mut families: Vec<String> = text
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split(',').next())
            .map(|family| family.trim().to_owned())
            .filter(|family| !family.is_empty())
            .collect();
        families.sort_by_key(|family| family.to_lowercase());
        families.dedup();
        families
    };
    let (all, mono) = tokio::join!(
        query("fc-list", [":", "family"]),
        query("fc-list", [":spacing=100", "family"]),
    );
    (parse(all), parse(mono))
}

const IMAGE_TYPES: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| IMAGE_TYPES.contains(&extension.to_lowercase().as_str()))
}

/// Folders searched for wallpapers.
pub fn wallpaper_dirs() -> Vec<PathBuf> {
    [
        home().join("Pictures/Wallpapers"),
        home().join("Pictures/wallpapers"),
        home().join("Wallpapers"),
        home().join(".local/share/wallpapers"),
        PathBuf::from("/usr/share/wallpapers/cachyos-wallpapers"),
        PathBuf::from("/usr/share/backgrounds"),
    ]
    .into_iter()
    .filter(|dir| dir.is_dir())
    .collect()
}

pub fn wallpapers() -> Vec<PathBuf> {
    let mut images: Vec<_> = wallpaper_dirs()
        .iter()
        .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| is_image(path))
        .collect();
    images.sort();
    images.dedup();
    images
}

/// A decoded preview.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Loads a small preview, decoding the original only once.
///
/// Wallpapers are often 5K PNGs that take a good part of a second to decode, so
/// previews are kept on disk. The key includes size and modification time, and
/// replacing the file therefore refreshes the preview.
pub fn thumbnail(path: &Path) -> Option<Thumbnail> {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let metadata = std::fs::metadata(path).ok()?;
    let mut hasher = DefaultHasher::new();
    (path, metadata.len(), metadata.modified().ok()).hash(&mut hasher);
    let cached = super::cache_dir()
        .join("hyprkontrol/thumbnails")
        .join(format!("{:016x}.jpg", hasher.finish()));

    let preview = match image::open(&cached) {
        Ok(preview) => preview,
        Err(_) => {
            let preview = image::open(path).ok()?.resize_to_fill(
                400,
                225,
                image::imageops::FilterType::Triangle,
            );
            if let Some(dir) = cached.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = preview.to_rgb8().save(&cached);
            preview
        }
    };
    let preview = preview.to_rgba8();
    Some(Thumbnail { width: preview.width(), height: preview.height(), pixels: preview.into_raw() })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wallpaper {
    pub path: String,
    pub fit: String,
}

fn paper_conf() -> PathBuf {
    config_dir().join("hypr/hyprpaper.conf")
}

fn conf_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

pub async fn wallpaper() -> Wallpaper {
    let text = std::fs::read_to_string(paper_conf()).unwrap_or_default();
    let active = query("hyprctl", ["hyprpaper", "listactive"]).await;
    Wallpaper {
        path: active
            .and_then(|text| Some(text.lines().next()?.split_once(": ")?.1.trim().to_owned()))
            .or_else(|| conf_value(&text, "path"))
            .unwrap_or_default(),
        fit: conf_value(&text, "fit_mode").unwrap_or_else(|| "cover".to_owned()),
    }
}

pub fn set_wallpaper(before: &Wallpaper, after: &Wallpaper) -> Change {
    let write = |paper: &Wallpaper| Op::Write {
        path: paper_conf(),
        text: format!(
            "wallpaper {{\n    monitor  =\n    path     = {}\n    fit_mode = {}\n}}\n",
            paper.path, paper.fit
        ),
        then: vec![Cmd::new("hyprctl").args([
            "hyprpaper".to_owned(),
            "wallpaper".to_owned(),
            format!(",{},{}", paper.path, paper.fit),
        ])],
    };
    Change::new(write(after)).undo(write(before))
}

/// An installed application, as described by its desktop entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub mime: Vec<String>,
}

impl std::fmt::Display for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

fn entry(text: &str, key: &str) -> Option<String> {
    edit::ini_get(text, "Desktop Entry", key).map(str::to_owned)
}

pub fn apps() -> Vec<App> {
    let mut apps: BTreeMap<String, App> = BTreeMap::new();
    // The user's entries come last so that they replace system entries of the
    // same id, which is how the specification defines precedence.
    for dir in [PathBuf::from("/usr/share/applications"), home().join(".local/share/applications")]
    {
        for path in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()) {
            let Some(id) = path.file_name().and_then(|name| name.to_str()) else { continue };
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Some(name) = entry(&text, "Name") else { continue };
            let mime = entry(&text, "MimeType").unwrap_or_default();
            apps.insert(
                id.to_owned(),
                App {
                    id: id.to_owned(),
                    name,
                    mime: mime.split(';').filter(|m| !m.is_empty()).map(str::to_owned).collect(),
                },
            );
        }
    }
    let mut apps: Vec<_> = apps.into_values().collect();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

/// A kind of file the user thinks of as one choice. The first type decides which
/// applications qualify and all of them are assigned together.
pub struct Role {
    pub label: &'static str,
    pub mime: &'static [&'static str],
}

pub const ROLES: &[Role] = &[
    Role { label: "Web", mime: &["text/html", "x-scheme-handler/http", "x-scheme-handler/https"] },
    Role { label: "Mail", mime: &["x-scheme-handler/mailto"] },
    Role { label: "Files", mime: &["inode/directory"] },
    Role { label: "Text", mime: &["text/plain"] },
    Role { label: "Photos", mime: &["image/png", "image/jpeg", "image/webp", "image/gif"] },
    Role { label: "Music", mime: &["audio/mpeg", "audio/flac", "audio/ogg"] },
    Role { label: "Video", mime: &["video/mp4", "video/x-matroska", "video/webm"] },
    Role { label: "Documents", mime: &["application/pdf"] },
    Role { label: "Archives", mime: &["application/zip", "application/x-tar"] },
];

pub async fn default_app(mime: &str) -> String {
    query("xdg-mime", ["query", "default", mime]).await.unwrap_or_default().trim().to_owned()
}

pub fn set_default_app(role: &Role, before: &str, after: &str) -> Change {
    let set = |id: &str| Cmd::new("xdg-mime").args(["default", id]).args(role.mime.iter().copied());
    let change = Change::new(set(after));
    if before.is_empty() { change } else { change.undo(set(before)) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Startup {
    pub file: String,
    pub name: String,
    pub comment: String,
    pub enabled: bool,
    pub system: bool,
}

/// Startup entries, with the user's copy taking precedence over the system's.
pub fn startup() -> Vec<Startup> {
    let mut entries: BTreeMap<String, Startup> = BTreeMap::new();
    for (dir, system) in
        [(PathBuf::from("/etc/xdg/autostart"), true), (config_dir().join("autostart"), false)]
    {
        for path in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()) {
            let Some(file) = path.file_name().and_then(|name| name.to_str()) else { continue };
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if !file.ends_with(".desktop") {
                continue;
            }
            let previous = entries.remove(file);
            entries.insert(
                file.to_owned(),
                Startup {
                    name: entry(&text, "Name")
                        .or_else(|| previous.map(|p| p.name))
                        .unwrap_or_else(|| file.to_owned()),
                    comment: entry(&text, "Comment").unwrap_or_default(),
                    enabled: entry(&text, "Hidden").as_deref() != Some("true"),
                    file: file.to_owned(),
                    system,
                },
            );
        }
    }
    let mut entries: Vec<_> = entries.into_values().collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

/// Enables or disables an entry by writing `Hidden` to the user's copy. System
/// files are never modified.
pub fn set_startup(file: &str, enabled: bool) -> Change {
    let path = config_dir().join("autostart").join(file);
    let text = std::fs::read_to_string(&path)
        .or_else(|_| std::fs::read_to_string(Path::new("/etc/xdg/autostart").join(file)))
        .unwrap_or_default();
    let value = if enabled { "false" } else { "true" };
    Change::write(path, edit::ini_set(&text, "Desktop Entry", "Hidden", value), vec![])
}

pub fn add_startup(app: &App) -> Result<Change> {
    let source =
        [home().join(".local/share/applications"), PathBuf::from("/usr/share/applications")]
            .into_iter()
            .map(|dir| dir.join(&app.id))
            .find(|path| path.is_file())
            .ok_or_else(|| Error::new(format!("{} has no desktop entry", app.name)))?;
    let text = std::fs::read_to_string(source)?;
    Ok(Change::write(
        config_dir().join("autostart").join(&app.id),
        edit::ini_set(&text, "Desktop Entry", "Hidden", "false"),
        vec![],
    ))
}

fn notifications_path() -> PathBuf {
    config_dir().join("swaync/config.json")
}

/// SwayNC configuration. Kept as JSON so that keys this program does not know
/// survive a round trip in their original order.
pub fn notifications() -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(notifications_path()).ok()?).ok()
}

pub fn set_notification(
    config: &serde_json::Value,
    key: &str,
    value: serde_json::Value,
) -> Result<Change> {
    let mut config = config.clone();
    config[key] = value;
    let text = serde_json::to_string_pretty(&config)? + "\n";
    let reload = Cmd::new("swaync-client").args(["--reload-config"]);
    Ok(Change::write(notifications_path(), text, vec![reload]))
}

/// `None` when SwayNC is not running.
pub async fn do_not_disturb() -> Option<bool> {
    query("swaync-client", ["--get-dnd", "--skip-wait"]).await.map(|text| text.trim() == "true")
}

pub fn set_do_not_disturb(on: bool) -> Change {
    let set = |on: bool| {
        let flag = if on { "--dnd-on" } else { "--dnd-off" };
        Cmd::new("swaync-client").args([flag, "--skip-wait"])
    };
    Change::new(set(on)).undo(set(!on))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gsettings() {
        let look = parse_look(
            "org.gnome.desktop.interface color-scheme 'prefer-dark'\n\
             org.gnome.desktop.interface font-name 'Adwaita Sans 11'\n\
             org.gnome.desktop.interface cursor-size 24\n\
             org.gnome.desktop.interface text-scaling-factor 1.25\n",
        );
        assert!(look.dark);
        assert_eq!(font_family(&look.font), "Adwaita Sans");
        assert_eq!((look.cursor_size, look.text_scale), (24, 1.25));
        assert_eq!(font_family("Inter"), "Inter");
    }

    #[test]
    fn notification_edits_keep_unknown_keys_in_order() {
        let config: serde_json::Value =
            serde_json::from_str(r#"{"zeta":1,"timeout":8,"alpha":[2]}"#).unwrap();
        let change = set_notification(&config, "timeout", 5.into()).unwrap();
        let Op::Write { text, .. } = change.apply else { panic!("expected a file write") };
        let (zeta, timeout, alpha) = (text.find("zeta"), text.find("timeout"), text.find("alpha"));
        assert!(zeta < timeout && timeout < alpha);
        assert!(text.contains("\"timeout\": 5"));
    }

    #[test]
    fn recognises_images() {
        assert!(is_image(Path::new("/a/B.PNG")));
        assert!(!is_image(Path::new("/a/b.txt")));
    }
}
