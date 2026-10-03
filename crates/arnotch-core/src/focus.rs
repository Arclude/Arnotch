//! Brings another application's window to the front.
//!
//! Wayland leaves window activation to the compositor, so each one needs its own way:
//! a KWin script on KDE Plasma, `hyprctl` on Hyprland and `swaymsg` on Sway.

use std::path::PathBuf;
use std::process::Command;

/// Focuses the most recently used window of process `pid`.
pub fn focus_window(pid: u32) -> anyhow::Result<()> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return run(
            "hyprctl",
            &["dispatch", "focuswindow", &format!("pid:{pid}")],
        )
        .map(drop);
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        return run("swaymsg", &[&format!("[pid={pid}]"), "focus"]).map(drop);
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if desktop.to_lowercase().contains("kde") {
        return kwin_focus(pid);
    }
    anyhow::bail!(crate::i18n::trf(
        "Switching windows is not supported on {} yet",
        &[&desktop]
    ))
}

const KWIN_SCRIPT_NAME: &str = "arnotch-focus";

fn kwin_focus(pid: u32) -> anyhow::Result<()> {
    let script = format!(
        "const w = workspace.stackingOrder.slice().reverse()\n\
         \x20   .find(w => w.pid === {pid} && w.normalWindow);\n\
         if (w) {{ w.minimized = false; workspace.activeWindow = w; }}\n"
    );
    let path = runtime_dir().join("arnotch-focus.js");
    std::fs::write(&path, script)?;

    // A script stays loaded under its name until unloaded; replace the previous one.
    _ = kwin(&[
        "/Scripting",
        "org.kde.kwin.Scripting.unloadScript",
        &format!("string:{KWIN_SCRIPT_NAME}"),
    ]);
    let reply = kwin(&[
        "/Scripting",
        "org.kde.kwin.Scripting.loadScript",
        &format!("string:{}", path.display()),
        &format!("string:{KWIN_SCRIPT_NAME}"),
    ])?;
    let id: i32 = reply
        .split_whitespace()
        .skip_while(|word| *word != "int32")
        .nth(1)
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("unexpected reply from KWin: {reply}"))?;
    kwin(&[&format!("/Scripting/Script{id}"), "org.kde.kwin.Script.run"]).map(drop)
}

fn kwin(args: &[&str]) -> anyhow::Result<String> {
    let mut all = vec!["--session", "--print-reply", "--dest=org.kde.KWin"];
    all.extend_from_slice(args);
    run("dbus-send", &all)
}

fn run(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from)
}
