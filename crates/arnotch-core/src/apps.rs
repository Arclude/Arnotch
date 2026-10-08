//! The installed apps, from their `.desktop` files, to open one by name: `arnotchctl
//! launch spotify` (Claude uses it to work the computer for the user).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// An app the user can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The desktop file id, such as "org.kde.dolphin" or "com.spotify.Client".
    pub id: String,
    pub name: String,
    /// Words that also find it: its name in the user's language, generic name, keywords.
    pub also: Vec<String>,
    pub path: PathBuf,
}

/// Where desktop files are, the user's first (they hide the system's with the same id).
fn dirs() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut dirs = vec![data_home.join("applications")];
    dirs.extend(
        [
            data_home.join("flatpak/exports/share"),
            PathBuf::from("/var/lib/flatpak/exports/share"),
        ]
        .into_iter()
        .chain(data_dirs.split(':').map(PathBuf::from))
        .map(|dir| dir.join("applications")),
    );
    let mut seen = Vec::new();
    dirs.retain(|dir| {
        let new = !seen.contains(dir);
        seen.push(dir.clone());
        new
    });
    dirs
}

/// Every app shown in menus, by name.
pub fn installed() -> Vec<App> {
    let language = crate::i18n::language();
    let mut apps: HashMap<String, App> = HashMap::new();
    for dir in dirs() {
        let mut files = Vec::new();
        collect(&dir, &mut files);
        for path in files {
            let Some(id) = path
                .strip_prefix(&dir)
                .ok()
                .and_then(|rel| rel.to_str())
                .and_then(|rel| rel.strip_suffix(".desktop"))
                .map(|rel| rel.replace('/', "-"))
            else {
                continue;
            };
            if apps.contains_key(&id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(app) = parse(&id, &text, language, path.clone()) {
                apps.insert(id, app);
            }
        }
    }
    let mut apps: Vec<App> = apps.into_values().collect();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            out.push(path);
        }
    }
}

/// An app from its desktop file, unless it is hidden or not an app.
fn parse(id: &str, text: &str, language: &str, path: PathBuf) -> Option<App> {
    let mut fields: HashMap<&str, &str> = HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry && let Some((key, value)) = line.split_once('=') {
            fields.entry(key.trim()).or_insert(value.trim());
        }
    }
    let field = |key: &str| fields.get(key).copied().unwrap_or_default();
    if field("Type") != "Application"
        || field("NoDisplay") == "true"
        || field("Hidden") == "true"
        || field("Name").is_empty()
    {
        return None;
    }
    let mut also = Vec::new();
    for key in ["Name", "GenericName", "Keywords"] {
        for key in [format!("{key}[{language}]"), key.to_owned()] {
            also.extend(
                field(&key)
                    .split(';')
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                    .map(str::to_owned),
            );
        }
    }
    let name = match field(&format!("Name[{language}]")) {
        "" => field("Name"),
        local => local,
    };
    Some(App {
        id: id.to_owned(),
        name: name.to_owned(),
        also,
        path,
    })
}

/// Lowercase without accents or spaces, so "Ayarlar", "ayarlar" and "Sistem Ayarları"
/// compare as a speaker would expect.
pub(crate) fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| match c {
            // "İ" lowercases to "i" and a combining dot.
            '\u{307}' => None,
            'ı' => Some('i'),
            'ş' => Some('s'),
            'ğ' => Some('g'),
            'ü' => Some('u'),
            'ö' => Some('o'),
            'ç' => Some('c'),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}

/// The apps matching `query`, best first.
pub fn find<'a>(apps: &'a [App], query: &str) -> Vec<&'a App> {
    let query = fold(query);
    if query.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u32, &App)> = apps
        .iter()
        .filter_map(|app| {
            let name = fold(&app.name);
            let id = fold(&app.id);
            let score = if name == query || id.rsplit('.').next() == Some(&query) {
                100
            } else if name.starts_with(&query) {
                80
            } else if name.contains(&query) {
                60
            } else if id.contains(&query) {
                50
            } else if app.also.iter().any(|word| fold(word).contains(&query)) {
                40
            } else {
                return None;
            };
            Some((score, app))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.len().cmp(&b.1.name.len())));
    scored.into_iter().map(|(_, app)| app).collect()
}

/// Opens the app, without waiting for it.
pub fn launch(app: &App) -> anyhow::Result<()> {
    let quiet = |command: &mut Command| {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    if quiet(Command::new("gtk-launch").arg(&app.id)).is_ok_and(|s| s.success()) {
        return Ok(());
    }
    // KDE's own way, where gtk-launch is missing
    if quiet(Command::new("kioclient").arg("exec").arg(&app.path)).is_ok_and(|s| s.success()) {
        return Ok(());
    }
    anyhow::bail!("cannot start {} ({})", app.name, app.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, text: &str) -> App {
        parse(id, text, "tr", PathBuf::new()).unwrap()
    }

    #[test]
    fn reads_desktop_files() {
        let settings = app(
            "systemsettings",
            "[Desktop Entry]\nType=Application\nName=System Settings\nName[tr]=Sistem Ayarları\nKeywords=config;\n[Desktop Action x]\nName=Other\n",
        );
        assert_eq!(settings.name, "Sistem Ayarları");
        assert!(settings.also.contains(&"System Settings".to_owned()));
        assert!(settings.also.contains(&"config".to_owned()));
        assert!(
            parse(
                "x",
                "[Desktop Entry]\nType=Application\nName=X\nNoDisplay=true\n",
                "tr",
                PathBuf::new()
            )
            .is_none()
        );
        assert!(
            parse(
                "x",
                "[Desktop Entry]\nType=Link\nName=X\n",
                "tr",
                PathBuf::new()
            )
            .is_none()
        );
    }

    #[test]
    fn finds_apps_as_spoken() {
        let apps = vec![
            app(
                "com.spotify.Client",
                "[Desktop Entry]\nType=Application\nName=Spotify\n",
            ),
            app(
                "org.kde.dolphin",
                "[Desktop Entry]\nType=Application\nName=Dolphin\nGenericName[tr]=Dosya Yöneticisi\n",
            ),
            app(
                "systemsettings",
                "[Desktop Entry]\nType=Application\nName=System Settings\nName[tr]=Sistem Ayarları\n",
            ),
            app(
                "org.kde.spectacle",
                "[Desktop Entry]\nType=Application\nName=Spectacle\n",
            ),
        ];
        let names = |q: &str| {
            find(&apps, q)
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("spotify")[0], "Spotify");
        assert_eq!(names("SPOTİFY")[0], "Spotify");
        assert_eq!(names("dosya yöneticisi"), ["Dolphin"]);
        assert_eq!(names("ayarlar"), ["Sistem Ayarları"]);
        assert_eq!(names("sistem ayarları"), ["Sistem Ayarları"]);
        assert_eq!(names("sp"), ["Spotify", "Spectacle"]);
        assert!(names("gimp").is_empty());
    }
}
