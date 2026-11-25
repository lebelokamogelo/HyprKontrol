//! Finds themes on disk, resolves inheritance and writes edits back.
//!
//! A broken user file must never keep the program from starting. Problems are
//! collected in [`Library::errors`] and shown in the Theme panel, while every
//! other theme stays usable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::spec::{File, Spec};
use crate::sys::{Error, Result, config_dir};

const BUILT_IN: &[(&str, &str)] = &[
    ("cachyos-dark", include_str!("../../themes/cachyos-dark.toml")),
    ("cachyos-light", include_str!("../../themes/cachyos-light.toml")),
    ("glass", include_str!("../../themes/glass.toml")),
    ("high-contrast", include_str!("../../themes/high-contrast.toml")),
    ("terminal", include_str!("../../themes/terminal.toml")),
];

/// The root of the inheritance tree, and the fallback when a theme goes missing.
pub const DEFAULT: &str = "cachyos-dark";

struct Source {
    file: File,
    path: Option<PathBuf>,
}

/// All themes available to the user.
#[derive(Debug, Clone, Default)]
pub struct Library {
    pub themes: Vec<Spec>,
    /// Theme files that could not be loaded, with the reason.
    pub errors: Vec<(String, String)>,
}

/// Directory holding user themes.
pub fn dir() -> PathBuf {
    config_dir().join("hyprkontrol/themes")
}

impl Library {
    pub fn load(dir: &Path) -> Self {
        let mut errors = Vec::new();
        let mut sources: BTreeMap<String, Source> = BTreeMap::new();

        for (id, text) in BUILT_IN {
            let file = toml::from_str(text).expect("built-in themes are valid");
            sources.insert((*id).to_owned(), Source { file, path: None });
        }

        for path in files(dir) {
            let id = theme_id(&path);
            let parsed = if sources.contains_key(&id) {
                Err(format!("\"{id}\" is already taken, rename the file"))
            } else {
                std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|text| toml::from_str::<File>(&text).map_err(|e| describe(&e)))
            };
            match parsed {
                Ok(file) => {
                    sources.insert(id, Source { file, path: Some(path) });
                }
                Err(reason) => errors.push((file_label(&path), reason)),
            }
        }

        let mut themes = Vec::new();
        for id in sources.keys() {
            match resolve(id, &sources, &mut Vec::new()) {
                Ok(spec) => themes.push(spec),
                Err(reason) => errors.push((id.clone(), reason)),
            }
        }
        // Built-in themes first, in their declared order.
        themes.sort_by_key(|spec| {
            BUILT_IN.iter().position(|(id, _)| *id == spec.id).unwrap_or(BUILT_IN.len())
        });
        Self { themes, errors }
    }

    pub fn get(&self, id: &str) -> Option<&Spec> {
        self.themes.iter().find(|spec| spec.id == id)
    }

    /// Returns the theme, or the default when it does not exist.
    pub fn get_or_default(&self, id: &str) -> &Spec {
        self.get(id).or_else(|| self.get(DEFAULT)).expect("the default theme is built in")
    }

    /// Derives an id from `name` that no existing theme uses.
    pub fn unique_id(&self, name: &str) -> String {
        let slug = slug(name);
        (1..)
            .map(|n| if n == 1 { slug.clone() } else { format!("{slug}-{n}") })
            .find(|id| self.get(id).is_none())
            .expect("an unused suffix always exists")
    }

    /// Writes `spec` as a user theme and returns its path.
    pub fn save(&self, dir: &Path, spec: &Spec) -> Result<PathBuf> {
        let base = self.get_or_default(spec.base.as_deref().unwrap_or(DEFAULT));
        let text = toml::to_string(&spec.to_file(base))
            .map_err(|e| Error::new(format!("Could not encode theme: {e}")))?;
        let path = spec.path.clone().unwrap_or_else(|| dir.join(format!("{}.toml", spec.id)));
        crate::sys::edit::write_atomic(&path, &text)?;
        Ok(path)
    }

    /// Parses a theme that has not been installed, for checking it.
    pub fn check(&self, path: &Path) -> Result<Spec> {
        let text = std::fs::read_to_string(path)?;
        let file: File = toml::from_str(&text).map_err(|e| Error::new(describe(&e)))?;
        let base = file.base.as_deref().unwrap_or(DEFAULT);
        let base = self
            .get(base)
            .ok_or_else(|| Error::new(format!("base theme \"{base}\" does not exist")))?;
        Ok(base.clone().extend(&theme_id(path), &file, Some(path.to_owned())))
    }

    /// Copies an external theme file into the user directory.
    pub fn import(&self, dir: &Path, source: &Path) -> Result<String> {
        let text = std::fs::read_to_string(source)?;
        let file: File = toml::from_str(&text).map_err(|e| Error::new(describe(&e)))?;
        let id = self.unique_id(&file.name);
        crate::sys::edit::write_atomic(&dir.join(format!("{id}.toml")), &text)?;
        Ok(id)
    }
}

/// Lists `*.toml` and `*/theme.toml` below `dir`.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| {
            if path.is_dir() {
                Some(path.join("theme.toml")).filter(|theme| theme.is_file())
            } else {
                (path.extension()? == "toml").then_some(path)
            }
        })
        .collect();
    files.sort();
    files
}

fn theme_id(path: &Path) -> String {
    let stem = match path.file_name().and_then(|name| name.to_str()) {
        Some("theme.toml") => path.parent().and_then(Path::file_name),
        _ => path.file_stem(),
    };
    slug(&stem.unwrap_or_default().to_string_lossy())
}

fn file_label(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// Reduces `name` to lowercase ASCII words joined by dashes.
fn slug(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if words.is_empty() { "theme".to_owned() } else { words.join("-") }
}

/// Shortens a TOML error to a single line with its location.
fn describe(error: &toml::de::Error) -> String {
    error.to_string().lines().filter(|line| !line.contains('|')).collect::<Vec<_>>().join(", ")
}

fn resolve(
    id: &str,
    sources: &BTreeMap<String, Source>,
    visiting: &mut Vec<String>,
) -> std::result::Result<Spec, String> {
    if visiting.iter().any(|seen| seen == id) {
        return Err(format!("base themes form a loop: {} > {id}", visiting.join(" > ")));
    }
    let source = sources.get(id).ok_or_else(|| format!("base theme \"{id}\" does not exist"))?;
    visiting.push(id.to_owned());
    let base = match source.file.base.as_deref() {
        Some(base) => resolve(base, sources, visiting)?,
        None if id == DEFAULT => Spec::default(),
        None => resolve(DEFAULT, sources, visiting)?,
    };
    Ok(base.extend(id, &source.file, source.path.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::color::{Hex, READABLE};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hyprkontrol-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn built_in_themes_resolve_and_are_readable() {
        let library = Library::load(Path::new("/nonexistent"));
        assert_eq!(library.themes.len(), BUILT_IN.len());
        assert!(library.errors.is_empty(), "{:?}", library.errors);
        assert_eq!(library.themes[0].id, DEFAULT);
        for spec in &library.themes {
            for (label, ratio) in spec.colors.readability() {
                assert!(ratio >= READABLE, "{}: {label} is {ratio:.1}", spec.id);
            }
        }
    }

    #[test]
    fn user_theme_inherits_from_base() {
        let dir = scratch("inherit");
        std::fs::write(
            dir.join("mine.toml"),
            "name = \"Mine\"\nbase = \"cachyos-light\"\n[colors]\naccent = \"#ff0000\"\n",
        )
        .unwrap();
        let library = Library::load(&dir);
        let mine = library.get("mine").unwrap();
        let light = library.get("cachyos-light").unwrap();
        assert_eq!(Hex(mine.colors.accent).to_string(), "#ff0000");
        assert_eq!(mine.colors.background, light.colors.background);
        assert_eq!(mine.metrics, light.metrics);
    }

    #[test]
    fn broken_files_are_reported_not_fatal() {
        let dir = scratch("broken");
        std::fs::write(dir.join("typo.toml"), "name = \"T\"\n[colors]\nacent = \"#fff\"\n")
            .unwrap();
        std::fs::write(dir.join("bad.toml"), "name = \"B\"\n[colors]\naccent = \"red\"\n").unwrap();
        std::fs::write(dir.join("a.toml"), "name = \"A\"\nbase = \"b\"\n").unwrap();
        std::fs::write(dir.join("b.toml"), "name = \"B\"\nbase = \"a\"\n").unwrap();
        std::fs::write(dir.join("glass.toml"), "name = \"Glass\"\n").unwrap();
        let library = Library::load(&dir);
        assert_eq!(library.themes.len(), BUILT_IN.len());
        assert_eq!(library.errors.len(), 5, "{:?}", library.errors);
    }

    #[test]
    fn saving_writes_only_differences() {
        let dir = scratch("save");
        let library = Library::load(&dir);
        let mut spec = library.get(DEFAULT).unwrap().clone();
        spec.id = library.unique_id("My Theme!");
        spec.name = "My Theme!".into();
        spec.base = Some(DEFAULT.into());
        spec.path = None;
        spec.metrics.card_radius = 4.0;
        let path = library.save(&dir, &spec).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("card_radius"));
        assert!(!text.contains("accent"), "unchanged values must be inherited:\n{text}");
        let reloaded = Library::load(&dir);
        assert_eq!(reloaded.get("my-theme").unwrap().metrics, spec.metrics);
        assert_eq!(reloaded.unique_id("My Theme"), "my-theme-2");
    }
}
