//! System settings for CachyOS on Hyprland.

mod app;
mod config;
mod icon;
mod panels;
mod sys;
mod theme;
mod ui;

use app::App;
use panels::Id;

const USAGE: &str = "Usage: hyprkontrol [PANEL]

Opens the settings window, on PANEL if one is given.

  --list              List the available panels
  --check-theme FILE  Check a theme file and its readability
  --help              Show this help
  --version           Show the version";

fn main() -> iced::Result {
    let mut panel = None;
    if let Some(argument) = std::env::args().nth(1) {
        match argument.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("hyprkontrol {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--list" => {
                for id in Id::ALL {
                    println!("{:<14} {}", id.key(), id.title());
                }
                return Ok(());
            }
            "--check-theme" => {
                let Some(path) = std::env::args_os().nth(2) else {
                    eprintln!("Name the theme file to check.");
                    std::process::exit(2);
                };
                std::process::exit(check_theme(path.as_ref()));
            }
            key => {
                panel = Id::from_key(key);
                if panel.is_none() {
                    eprintln!("There is no panel called \"{key}\". Try --list.");
                    std::process::exit(2);
                }
            }
        }
    }

    theme::set_system_family(&system_font());
    iced::application(move || App::new(panel), App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(1180.0, 780.0),
            min_size: Some(iced::Size::new(340.0, 420.0)),
            // Lets translucent themes show the desktop through.
            transparent: true,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "hyprkontrol".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        })
        .antialiasing(true)
        .run()
}

/// Returns the exit code: 0 when the theme is fine, 1 when it needs work.
fn check_theme(path: &std::path::Path) -> i32 {
    let library = theme::Library::load(&theme::library::dir());
    let spec = match library.check(path) {
        Ok(spec) => spec,
        Err(error) => {
            eprintln!("{}: {error}", path.display());
            return 1;
        }
    };
    println!("{} loads.", spec.name);
    let mut code = 0;
    for (label, ratio) in spec.colors.readability() {
        let verdict = if ratio >= theme::color::READABLE {
            "ok"
        } else {
            code = 1;
            "hard to read, aim for 4.5"
        };
        println!("  {label:<24} {ratio:>5.1}  {verdict}");
    }
    for (name, icon) in &spec.icons {
        if icon::Icon::from_name(name).is_none() {
            println!("  There is no icon called \"{name}\".");
            code = 1;
        } else if !icon.is_file() {
            println!("  {} does not exist.", icon.display());
            code = 1;
        }
    }
    code
}

/// The desktop's interface font. Read once and before the window opens, since
/// text is measured with it from the first frame.
fn system_font() -> String {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "font-name"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().trim_matches('\'').to_owned())
        .filter(|text| !text.is_empty())
        .map(|text| sys::desktop::font_family(&text).to_owned())
        .unwrap_or_else(|| "Adwaita Sans".to_owned())
}
