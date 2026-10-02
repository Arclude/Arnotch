//! Uploads a file to a sharing service and returns its public link, with progress.
//!
//! Uses `curl`, so no HTTP stack is needed and any service with a curl recipe works.
//! Files leave the computer here: callers only upload what the user asked to share,
//! to the service the user picked.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

use crate::i18n::{tr, trf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Service {
    /// <https://0x0.st>: kept 30 days to 1 year depending on size.
    ZeroX0,
    /// <https://catbox.moe>: permanent.
    Catbox,
    /// <https://litterbox.catbox.moe>: deleted after 24 hours.
    Litterbox,
    /// A shell command; `{file}` is replaced by the quoted path and the link is
    /// the last line it prints.
    Custom(String),
}

impl Service {
    /// The service picked in the settings; `None` when sharing is off.
    pub fn from_setting(name: &str, command: &str) -> Option<Self> {
        match name {
            "0x0" => Some(Self::ZeroX0),
            "catbox" => Some(Self::Catbox),
            "litterbox" => Some(Self::Litterbox),
            "custom" if !command.trim().is_empty() => Some(Self::Custom(command.to_owned())),
            _ => None,
        }
    }
}

/// Uploads `path`, reporting progress from 0.0 to 1.0, and returns the link.
pub fn upload(
    path: &Path,
    service: &Service,
    mut progress: impl FnMut(f32),
) -> anyhow::Result<String> {
    if !path.is_file() {
        bail!(trf("{} is not a file", &[&path.display()]));
    }
    let mut command = match service {
        Service::Custom(template) => {
            let mut command = Command::new("sh");
            command
                .arg("-c")
                .arg(template.replace("{file}", &shell_quote(path)));
            command
        }
        _ => curl_command(path, service),
    };
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(tr("Cannot run curl"))?;

    // curl's progress bar redraws one line with \r: "#####    45.3%".
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let mut errors = String::new();
    let mut chunk = [0u8; 512];
    let mut line = String::new();
    while let Ok(n) = stderr.read(&mut chunk) {
        if n == 0 {
            break;
        }
        for c in String::from_utf8_lossy(&chunk[..n]).chars() {
            if c == '\r' || c == '\n' {
                if let Some(percent) = parse_percent(&line) {
                    progress(percent / 100.0);
                } else if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
                    errors.push_str(line.trim());
                    errors.push('\n');
                }
                line.clear();
            } else {
                line.push(c);
            }
        }
    }

    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let link = stdout
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or_default();
    if !output.status.success() || !link.starts_with("http") {
        let reason = if link.is_empty() { errors.trim() } else { link };
        let reason = if reason.is_empty() {
            tr("no link returned")
        } else {
            reason
        };
        bail!(trf("Upload failed: {}", &[&reason]));
    }
    progress(1.0);
    Ok(link.to_owned())
}

fn curl_command(path: &Path, service: &Service) -> Command {
    // Quoted so commas and semicolons in names aren't read as curl -F options.
    let file = format!(
        "@\"{}\"",
        path.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    );
    let mut command = Command::new("curl");
    command.args([
        "--progress-bar",
        "--fail-with-body",
        "--user-agent",
        "notch",
    ]);
    match service {
        Service::ZeroX0 => command.args(["-F", &format!("file={file}"), "https://0x0.st"]),
        Service::Catbox => command.args([
            "-F",
            "reqtype=fileupload",
            "-F",
            &format!("fileToUpload={file}"),
            "https://catbox.moe/user/api.php",
        ]),
        Service::Litterbox => command.args([
            "-F",
            "reqtype=fileupload",
            "-F",
            "time=24h",
            "-F",
            &format!("fileToUpload={file}"),
            "https://litterbox.catbox.moe/resources/internals/api.php",
        ]),
        Service::Custom(_) => unreachable!("custom services run through sh"),
    };
    command
}

fn parse_percent(line: &str) -> Option<f32> {
    let number = line.trim().strip_suffix('%')?.rsplit([' ', '#']).next()?;
    number.parse().ok()
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_curl_progress() {
        assert_eq!(
            parse_percent("###########                       45.3%"),
            Some(45.3)
        );
        assert_eq!(
            parse_percent("######################################## 100.0%"),
            Some(100.0)
        );
        assert_eq!(parse_percent("curl: (6) Could not resolve host"), None);
    }

    #[test]
    fn custom_commands_get_a_quoted_path() {
        let path = Path::new("/tmp/it's here.pdf");
        assert_eq!(shell_quote(path), r"'/tmp/it'\''s here.pdf'");
    }

    #[test]
    fn custom_service_uploads_through_the_shell() {
        let dir = std::env::temp_dir().join(format!("notch-share-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a b.txt");
        std::fs::write(&file, "hi").unwrap();

        let service = Service::Custom("test -f {file} && echo https://example.com/x".into());
        let mut seen = Vec::new();
        let link = upload(&file, &service, |p| seen.push(p)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(link, "https://example.com/x");
        assert_eq!(seen.last(), Some(&1.0));
    }

    #[test]
    fn sharing_is_off_unless_chosen() {
        assert_eq!(Service::from_setting("off", ""), None);
        assert_eq!(Service::from_setting("custom", "  "), None);
        assert_eq!(Service::from_setting("0x0", ""), Some(Service::ZeroX0));
    }
}
