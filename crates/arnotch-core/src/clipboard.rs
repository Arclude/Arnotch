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
