//! Types into terminals and switches to their tabs, through each terminal's own
//! remote-control interface: Konsole's D-Bus API, tmux, WezTerm's CLI, kitty's
//! remote control and Arterm's control socket. (Linux no longer lets one program
//! inject keystrokes into another's terminal, so there is no generic way.)

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, bail};

use crate::focus;
use crate::i18n::{tr, trf};
use crate::terminals::{self, Terminal};

/// Whether text can be sent to this terminal.
pub fn can_send(app: &str, app_pid: u32) -> bool {
    match app {
        "Konsole" | "tmux" | "WezTerm" | "kitty" => true,
        // Only Arterm versions with the control socket.
        "Arterm" => arterm_socket(app_pid).exists(),
        _ => false,
    }
}

/// Types `text` into the terminal, followed by Enter when `submit` is set.
///
/// Enter goes separately, a moment later: programs such as Claude Code treat text
/// arriving in one burst as a paste, where Enter only adds a new line.
pub fn send_text(terminal: &Terminal, text: &str, submit: bool) -> anyhow::Result<()> {
    type_text(terminal, text)?;
    if submit {
        std::thread::sleep(Duration::from_millis(80));
        press_enter(terminal)?;
    }
    Ok(())
}

fn type_text(terminal: &Terminal, text: &str) -> anyhow::Result<()> {
    match terminal.app.as_str() {
        "Konsole" => konsole_send(terminal, text),
        // -l sends the text literally instead of as key names.
        "tmux" => run(
            "tmux",
            &["send-keys", "-t", &tmux_pane(terminal)?, "-l", "--", text],
        )
        .map(drop),
        "WezTerm" => wezterm_send(terminal, text),
        "kitty" => kitty_send(terminal, text),
        "Arterm" => arterm_request(
            terminal,
            serde_json::json!({
                "cmd": "write",
                "shell_pid": terminal.shell_pid,
                "text": text,
            }),
        ),
        app => bail!(trf("Sending text to {} is not supported", &[&app])),
    }
}

fn press_enter(terminal: &Terminal) -> anyhow::Result<()> {
    match terminal.app.as_str() {
        "tmux" => run("tmux", &["send-keys", "-t", &tmux_pane(terminal)?, "Enter"]).map(drop),
        _ => type_text(terminal, "\r"),
    }
}

fn konsole_send(terminal: &Terminal, text: &str) -> anyhow::Result<()> {
    let (service, session) = konsole_session(terminal)?;
    let path = format!("/Sessions/{session}");
    dbus(&service, &path, "org.kde.konsole.Session.sendText", &[&format!("string:{text}")])
        .map(drop)
        .map_err(|err| {
            if err.to_string().contains("AccessDenied") {
                anyhow::anyhow!(tr(
                    "Konsole blocks typing from other apps. Enable it in Konsole: Settings › Configure Konsole › General › “Enable the security sensitive parts of the DBus API”"
                ))
            } else {
                err
            }
        })
}

fn wezterm_send(terminal: &Terminal, text: &str) -> anyhow::Result<()> {
    let pane = wezterm_pane(terminal)?;
    run(
        "wezterm",
        &[
            "cli",
            "send-text",
            "--pane-id",
            &pane,
            "--no-paste",
            "--",
            text,
        ],
    )
    .map(drop)
}

fn kitty_send(terminal: &Terminal, text: &str) -> anyhow::Result<()> {
    let socket = kitty_socket(terminal)?;
    let target = format!("pid:{}", terminal.shell_pid);
    run(
        "kitty",
        &[
            "@",
            "--to",
            &socket,
            "send-text",
            "--match",
            &target,
            "--",
            text,
        ],
    )
    .map(drop)
}

/// Brings the terminal's window to the front and selects the tab of this shell.
pub fn focus_tab(terminal: &Terminal) -> anyhow::Result<()> {
    match terminal.app.as_str() {
        "Konsole" => {
            let (service, session) = konsole_session(terminal)?;
            if let Some(window) = konsole_window_of(&service, session)? {
                dbus(
                    &service,
                    &format!("/Windows/{window}"),
                    "org.kde.konsole.Window.setCurrentSession",
                    &[&format!("int32:{session}")],
                )?;
            }
        }
        "tmux" => {
            let pane = tmux_pane(terminal)?;
            run("tmux", &["select-window", "-t", &pane])?;
            run("tmux", &["select-pane", "-t", &pane])?;
            // tmux has no window of its own: focus the terminal its client runs in.
            if let Some(app_pid) = tmux_client_terminal() {
                return focus::focus_window(app_pid);
            }
            return Ok(());
        }
        "WezTerm" => {
            let pane = wezterm_pane(terminal)?;
            run("wezterm", &["cli", "activate-pane", "--pane-id", &pane])?;
        }
        "kitty" => {
            let socket = kitty_socket(terminal)?;
            let target = format!("pid:{}", terminal.shell_pid);
            run(
                "kitty",
                &["@", "--to", &socket, "focus-window", "--match", &target],
            )?;
        }
        "Arterm" => {
            arterm_request(
                terminal,
                serde_json::json!({
                    "cmd": "focus",
                    "shell_pid": terminal.shell_pid,
                }),
            )?;
        }
        _ => {}
    }
    if terminal.app_pid == 0 {
        bail!("don't know which window {} runs in", terminal.id);
    }
    focus::focus_window(terminal.app_pid)
}

// Konsole: every process registers org.kde.konsole-<pid>; /Sessions/<n> are tabs
// and Session.processId() is the shell's pid.

fn konsole_session(terminal: &Terminal) -> anyhow::Result<(String, u32)> {
    let service = format!("org.kde.konsole-{}", terminal.app_pid);
    for session in dbus_children(&service, "/Sessions")? {
        let reply = dbus(
            &service,
            &format!("/Sessions/{session}"),
            "org.kde.konsole.Session.processId",
            &[],
        )?;
        if reply_int(&reply) == Some(terminal.shell_pid as i64) {
            return Ok((service, session));
        }
    }
    bail!("no Konsole tab runs shell {}", terminal.shell_pid)
}

fn konsole_window_of(service: &str, session: u32) -> anyhow::Result<Option<u32>> {
    for window in dbus_children(service, "/Windows")? {
        let reply = dbus(
            service,
            &format!("/Windows/{window}"),
            "org.kde.konsole.Window.sessionList",
            &[],
        )?;
        if reply.split('"').any(|s| s == session.to_string()) {
            return Ok(Some(window));
        }
    }
    Ok(None)
}

fn dbus(service: &str, path: &str, method: &str, args: &[&str]) -> anyhow::Result<String> {
    let dest = format!("--dest={service}");
    let mut all = vec!["--session", "--print-reply", dest.as_str(), path, method];
    all.extend_from_slice(args);
    run("dbus-send", &all)
}

/// Numeric child nodes of a D-Bus object path, e.g. the tabs under /Sessions.
fn dbus_children(service: &str, path: &str) -> anyhow::Result<Vec<u32>> {
    let xml = dbus(
        service,
        path,
        "org.freedesktop.DBus.Introspectable.Introspect",
        &[],
    )?;
    Ok(xml
        .split("<node name=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next()?.parse().ok())
        .collect())
}

/// The value of a reply such as `method return ... \n   int32 5180`.
fn reply_int(reply: &str) -> Option<i64> {
    let mut words = reply.split_whitespace();
    words.find(|w| w.starts_with("int") || w.starts_with("uint"))?;
    words.next()?.parse().ok()
}

// tmux, WezTerm and kitty identify panes by their own ids; match them by pty.

fn tmux_pane(terminal: &Terminal) -> anyhow::Result<String> {
    let panes = run(
        "tmux",
        &["list-panes", "-a", "-F", "#{pane_tty} #{pane_id}"],
    )?;
    let tty = format!("/dev/{}", terminal.id);
    panes
        .lines()
        .find_map(|line| {
            line.strip_prefix(&tty)?
                .trim()
                .strip_prefix('%')
                .map(|id| format!("%{id}"))
        })
        .with_context(|| format!("no tmux pane on {tty}"))
}

fn tmux_client_terminal() -> Option<u32> {
    let clients = run("tmux", &["list-clients", "-F", "#{client_pid}"]).ok()?;
    let pid = clients.lines().next()?.trim().parse().ok()?;
    terminals::terminal_app_of(pid).map(|(_, app_pid)| app_pid)
}

fn wezterm_pane(terminal: &Terminal) -> anyhow::Result<String> {
    let json = run("wezterm", &["cli", "list", "--format", "json"])?;
    let panes: Vec<serde_json::Value> = serde_json::from_str(&json)?;
    let tty = format!("/dev/{}", terminal.id);
    panes
        .iter()
        .find(|pane| pane["tty_name"] == tty.as_str())
        .and_then(|pane| pane["pane_id"].as_u64())
        .map(|id| id.to_string())
        .with_context(|| format!("no WezTerm pane on {tty}"))
}

// Arterm: a JSON-lines control socket in its bridge, keyed by the app's pid.

fn arterm_socket(app_pid: u32) -> PathBuf {
    let runtime =
        std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    runtime
        .join("arterm")
        .join(format!("control-{app_pid}.sock"))
}

fn arterm_request(terminal: &Terminal, request: serde_json::Value) -> anyhow::Result<()> {
    let path = arterm_socket(terminal.app_pid);
    let mut stream = UnixStream::connect(&path)
        .with_context(|| tr("Arterm's control socket is not available; update Arterm"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(format!("{request}\n").as_bytes())?;

    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    let reply: serde_json::Value =
        serde_json::from_str(&reply).context("unreadable reply from Arterm")?;
    if reply["ok"] == true {
        Ok(())
    } else {
        bail!(
            "Arterm: {}",
            reply["error"].as_str().unwrap_or("request failed")
        )
    }
}

/// kitty only accepts remote control on a socket set up with `listen_on`.
/// `NOTCH_KITTY_SOCKET` overrides the default `unix:@kitty-<pid>`.
fn kitty_socket(terminal: &Terminal) -> anyhow::Result<String> {
    if let Ok(socket) = std::env::var("NOTCH_KITTY_SOCKET") {
        return Ok(socket);
    }
    if terminal.app_pid == 0 {
        bail!("unknown kitty process");
    }
    Ok(format!("unix:@kitty-{}", terminal.app_pid))
}

fn run(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("cannot run {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_dbus_int_replies() {
        let reply = "method return time=1.2 sender=:1.5 -> destination=:1.9 serial=7 reply_serial=2\n   int32 5180\n";
        assert_eq!(reply_int(reply), Some(5180));
    }
}
