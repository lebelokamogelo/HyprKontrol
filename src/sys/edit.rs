//! Editing of configuration files that belong to the user.
//!
//! These files are often hand-written and under version control, so edits touch
//! the lines they have to and leave comments, order and spacing alone.

use std::fs;
use std::path::{Path, PathBuf};

use super::Result;

/// Replaces the contents of `path` without ever exposing a half-written file.
///
/// Dotfile managers commonly symlink configuration into a repository. Renaming
/// over the link would detach the file from that repository, so the write goes
/// to whatever the link points at.
pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut temporary = target.clone().into_os_string();
    temporary.push(".hyprkontrol.tmp");
    let temporary = PathBuf::from(temporary);

    fs::write(&temporary, text)?;
    if let Ok(metadata) = fs::metadata(&target) {
        fs::set_permissions(&temporary, metadata.permissions())?;
    }
    fs::rename(&temporary, &target)?;
    Ok(())
}

/// Keeps a copy of the file as it was before this program first touched it.
pub fn backup_once(path: &Path) -> Result<()> {
    let mut backup = path.as_os_str().to_owned();
    backup.push(".before-hyprkontrol");
    let backup = PathBuf::from(backup);
    if path.exists() && !backup.exists() {
        fs::copy(path, backup)?;
    }
    Ok(())
}

pub fn ini_get<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = name == section;
        } else if inside
            && let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim());
        }
    }
    None
}

/// Sets `key` in `section`, adding either one when it is missing.
pub fn ini_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let entry = format!("{key}={value}");
    let header = format!("[{section}]");

    let Some(start) = lines.iter().position(|line| line.trim() == header) else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.extend([header, entry]);
        return lines.join("\n") + "\n";
    };

    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| start + 1 + offset);
    let existing = lines[start + 1..end]
        .iter()
        .position(|line| line.split_once('=').is_some_and(|(name, _)| name.trim() == key));
    match existing {
        Some(offset) => lines[start + 1 + offset] = entry,
        None => {
            // Insert after the last entry, ahead of blank lines that separate sections.
            let last = lines[start..end].iter().rposition(|line| !line.trim().is_empty());
            lines.insert(start + last.unwrap_or(0) + 1, entry);
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "# mine\n[Settings]\ngtk-theme-name=Adwaita\n\n[Other]\na=1\n";

    #[test]
    fn replaces_and_inserts_without_disturbing_the_rest() {
        let text = ini_set(INI, "Settings", "gtk-theme-name", "adw-gtk3");
        assert_eq!(text, INI.replace("Adwaita", "adw-gtk3"));
        let text = ini_set(INI, "Settings", "gtk-font-name", "Inter 11");
        assert_eq!(
            text,
            "# mine\n[Settings]\ngtk-theme-name=Adwaita\ngtk-font-name=Inter 11\n\n[Other]\na=1\n"
        );
        assert_eq!(ini_set("", "New", "k", "v"), "[New]\nk=v\n");
        assert_eq!(ini_get(INI, "Other", "a"), Some("1"));
        assert_eq!(ini_get(INI, "Settings", "a"), None);
    }

    #[test]
    fn writes_through_symlinks() {
        let dir = std::env::temp_dir().join(format!("hyprkontrol-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (real, link) = (dir.join("real"), dir.join("link"));
        fs::write(&real, "old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, "new").unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
    }
}
