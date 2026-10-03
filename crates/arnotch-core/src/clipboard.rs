//! Puts text on the clipboard without needing keyboard focus.
//!
//! A Wayland client may only set the clipboard while it has focus, which the
//! island normally doesn't take. `wl-copy` (data-control protocol), `xclip` and
//! KDE's Klipper all work without it.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::bail;

pub fn copy(text: &str) -> anyhow::Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && pipe_to("wl-copy", &[], text).is_ok() {
        return Ok(());
    }
    if std::env::var_os("DISPLAY").is_some()
        && pipe_to("xclip", &["-selection", "clipboard"], text).is_ok()
    {
        return Ok(());
    }
    let status = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.kde.klipper",
            "/klipper",
            "org.kde.klipper.klipper.setClipboardContents",
            &format!("string:{text}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        bail!(crate::i18n::tr(
            "No clipboard tool found (install wl-clipboard)"
        ));
    }
    Ok(())
}

fn pipe_to(program: &str, args: &[&str], text: &str) -> anyhow::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(text.as_bytes())?;
    // wl-copy and xclip fork a helper that serves the clipboard; the parent exits.
    if !child.wait()?.success() {
        bail!("{program} failed");
    }
    Ok(())
}

/// How many text entries of the clipboard history the island shows.
const HISTORY: usize = 8;

/// An entry of the clipboard history.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub text: String,
    /// Looks like a private key or an access token: the island hides the text.
    pub secret: bool,
}

/// Calls `on_change` with the latest text entries of KDE's clipboard history (Klipper),
/// newest first, now and whenever it changes. Images are left out: Klipper can't hand
/// them back over D-Bus. Returns an error where Klipper doesn't run.
pub fn watch_history(on_change: impl Fn(Vec<Entry>)) -> zbus::Result<()> {
    use zbus::blocking::{Connection, MessageIterator};

    let conn = Connection::session()?;
    let read = || -> zbus::Result<Vec<Entry>> {
        let reply = conn.call_method(
            Some("org.kde.klipper"),
            "/klipper",
            Some("org.kde.klipper.klipper"),
            "getClipboardHistoryMenu",
            &(),
        )?;
        let items: Vec<String> = reply.body().deserialize()?;
        Ok(text_entries(items))
    };
    let mut last = read()?;
    on_change(last.clone());

    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.kde.klipper.klipper")?
        .member("clipboardHistoryUpdated")?
        .build();
    for message in MessageIterator::for_match_rule(rule, &conn, Some(16))? {
        message?;
        if let Ok(now) = read()
            && now != last
        {
            on_change(now.clone());
            last = now;
        }
    }
    Ok(())
}

/// Text entries only, without duplicates: Klipper lists images as "▨ 790 × 408".
fn text_entries(items: Vec<String>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for item in items {
        let image = item.starts_with("▨ ")
            && item[4..]
                .split(" × ")
                .all(|n| n.trim().parse::<u32>().is_ok());
        if !image && !item.trim().is_empty() && !out.iter().any(|e| e.text == item) {
            out.push(Entry {
                secret: looks_secret(&item),
                text: item,
            });
        }
        if out.len() == HISTORY {
            break;
        }
    }
    out
}

/// Private keys and the usual access token formats, so they are not shown on screen.
fn looks_secret(text: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "sk-",
        "sk_live_",
        "rk_live_",
        "ghp_",
        "gho_",
        "ghs_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "AKIA",
        "ASIA",
        "AIza",
        "ya29.",
        "hf_",
        "npm_",
        "pypi-",
    ];
    let text = text.trim();
    if text.contains("PRIVATE KEY-----") {
        return true;
    }
    let token = !text.contains(char::is_whitespace) && text.len() >= 20;
    let jwt = text.starts_with("eyJ") && text.matches('.').count() == 2;
    token && (jwt || PREFIXES.iter().any(|p| text.starts_with(p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_secrets() {
        assert!(looks_secret(
            "-----BEGIN PRIVATE KEY-----\nMIIEvAIBADAN\n-----END PRIVATE KEY-----"
        ));
        assert!(looks_secret(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk="
        ));
        assert!(looks_secret("ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"));
        assert!(looks_secret("sk-ant-api03-abcdefghijklmnopqrstuvwxyz"));
        assert!(looks_secret(
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl"
        ));
        assert!(!looks_secret("sk- is a prefix"));
        assert!(!looks_secret("https://share.example/devis-client.pdf"));
        assert!(!looks_secret("ls -la"));
    }

    #[test]
    fn keeps_text_entries_only() {
        let items = [
            "▨ 790 × 408",
            "ls -la",
            "",
            "▨ is a square",
            "ls -la",
            "hello",
        ]
        .map(String::from)
        .to_vec();
        let texts: Vec<String> = text_entries(items).into_iter().map(|e| e.text).collect();
        assert_eq!(texts, ["ls -la", "▨ is a square", "hello"]);
    }
}
