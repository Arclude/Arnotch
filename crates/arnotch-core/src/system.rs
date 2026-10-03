//! Volume, screen brightness and battery, for the island's indicators. All three are
//! event driven: `pactl subscribe` for volume, D-Bus signals for brightness (KDE's
//! ScreenBrightness service) and battery (UPower), so nothing is polled.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::OwnedValue;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Volume,
    Brightness,
}

/// The laptop's battery (UPower's display device).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Battery {
    /// From 0 to 100.
    pub percent: f64,
    /// Plugged in: charging, full or waiting to charge.
    pub plugged: bool,
}

/// Calls `on_change(volume 0..1, muted)` when the default output's volume or mute
/// changes. Runs until `pactl` goes away, so call it on its own thread.
pub fn watch_volume(on_change: impl Fn(f32, bool)) -> anyhow::Result<()> {
    let mut child = Command::new("pactl")
        .arg("subscribe")
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let lines = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();

    let mut last = read_volume();
    for line in lines {
        let line = line?;
        // "Event 'change' on sink #56", or "... on server" when the default output changes.
        if !line.contains("on sink #") && !line.contains("on server") {
            continue;
        }
        let now = read_volume();
        if now != last {
            if let Some((volume, muted)) = now {
                on_change(volume, muted);
            }
            last = now;
        }
    }
    _ = child.wait();
    Ok(())
}

fn read_volume() -> Option<(f32, bool)> {
    let pactl = |what: &str| -> Option<String> {
        // LC_ALL=C: "Mute: yes" is translated otherwise.
        let out = Command::new("pactl")
            .args([what, "@DEFAULT_SINK@"])
            .env("LC_ALL", "C")
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let volume = parse_volume(&pactl("get-sink-volume")?)?;
    let muted = pactl("get-sink-mute")?.trim().ends_with("yes");
    Some((volume, muted))
}

/// The first channel's percentage in `pactl get-sink-volume` output.
fn parse_volume(text: &str) -> Option<f32> {
    let percent = text.split('/').nth(1)?.trim().strip_suffix('%')?;
    Some(percent.trim().parse::<f32>().ok()? / 100.0)
}

/// Calls `on_change(brightness 0..1)` when a screen's brightness changes, through KDE's
/// ScreenBrightness service. Returns an error where that service doesn't exist.
pub fn watch_brightness(on_change: impl Fn(f32)) -> zbus::Result<()> {
    let conn = Connection::session()?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.kde.ScreenBrightness")?
        .member("BrightnessChanged")?
        .path("/org/kde/ScreenBrightness")?
        .build();
    let mut max: HashMap<String, i32> = HashMap::new();
    for message in MessageIterator::for_match_rule(rule, &conn, Some(16))? {
        let Ok((display, brightness, _, _)) = message?
            .body()
            .deserialize::<(String, i32, String, String)>()
        else {
            continue;
        };
        let max = match max.get(&display) {
            Some(max) => *max,
            None => {
                let value = display_max(&conn, &display).unwrap_or(0);
                max.insert(display, value);
                value
            }
        };
        if max > 0 {
            on_change((brightness as f32 / max as f32).clamp(0.0, 1.0));
        }
    }
    Ok(())
}

fn display_max(conn: &Connection, display: &str) -> Option<i32> {
    let reply = conn
        .call_method(
            Some("org.kde.ScreenBrightness"),
            format!("/org/kde/ScreenBrightness/{display}").as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.kde.ScreenBrightness.Display", "MaxBrightness"),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    i32::try_from(value).ok()
}

/// Calls `on_change` with the battery's state now and whenever it changes. Returns
/// right away on machines without a battery.
pub fn watch_battery(on_change: impl Fn(Battery)) -> zbus::Result<()> {
    const PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
    let conn = Connection::system()?;
    let read = || -> Option<Battery> {
        let reply = conn
            .call_method(
                Some("org.freedesktop.UPower"),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &("org.freedesktop.UPower.Device",),
            )
            .ok()?;
        let props: HashMap<String, OwnedValue> = reply.body().deserialize().ok()?;
        let get = |key: &str| props.get(key).and_then(|v| v.try_clone().ok());
        if !get("IsPresent")
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(false)
        {
            return None;
        }
        let percent = get("Percentage").and_then(|v| f64::try_from(v).ok())?;
        let state = get("State")
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        // 1 charging, 4 fully charged, 5 pending charge.
        Some(Battery {
            percent,
            plugged: matches!(state, 1 | 4 | 5),
        })
    };

    let Some(mut last) = read() else {
        return Ok(());
    };
    on_change(last);
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path(PATH)?
        .build();
    for message in MessageIterator::for_match_rule(rule, &conn, Some(16))? {
        message?;
        // UPower sends a few signals per update; read once they're all in.
        std::thread::sleep(Duration::from_millis(50));
        if let Some(now) = read()
            && now != last
        {
            on_change(now);
            last = now;
        }
    }
    Ok(())
}

/// A duration such as "25m", "1h30m", "90s" or "10" (minutes), in seconds.
pub fn parse_duration(text: &str) -> Option<u64> {
    let text = text.trim().to_ascii_lowercase();
    if let Ok(minutes) = text.parse::<u64>() {
        return Some(minutes * 60);
    }
    let (mut total, mut number) = (0u64, String::new());
    for c in text.chars() {
        match c {
            '0'..='9' => number.push(c),
            'h' | 'm' | 's' if !number.is_empty() => {
                let n: u64 = number.parse().ok()?;
                number.clear();
                total += n * match c {
                    'h' => 3600,
                    'm' => 60,
                    _ => 1,
                };
            }
            ' ' => {}
            _ => return None,
        }
    }
    (number.is_empty() && total > 0).then_some(total)
}

/// Plays a sound of the desktop's sound theme, such as "alarm-clock-elapsed", without
/// waiting for it.
pub fn play_sound(name: &str) {
    let themed = Command::new("canberra-gtk-play").args(["-i", name]).spawn();
    if themed.is_err() {
        _ = Command::new("paplay")
            .arg(format!("/usr/share/sounds/freedesktop/stereo/{name}.oga"))
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("25m"), Some(1500));
        assert_eq!(parse_duration("1h30m"), Some(5400));
        assert_eq!(parse_duration("90s"), Some(90));
        assert_eq!(parse_duration("10"), Some(600));
        assert_eq!(parse_duration("1h 5m"), Some(3900));
        assert_eq!(parse_duration("m"), None);
        assert_eq!(parse_duration("0"), Some(0));
        assert_eq!(parse_duration("soon"), None);
    }

    #[test]
    fn reads_pactl_volume() {
        let text = "Volume: front-left: 57016 /  87% / -3,63 dB,   front-right: 57016 /  87% / -3,63 dB\n        balance 0,00\n";
        assert_eq!(parse_volume(text), Some(0.87));
        assert_eq!(
            parse_volume("Volume: mono: 65536 / 100% / 0,00 dB"),
            Some(1.0)
        );
        assert_eq!(parse_volume("garbage"), None);
    }
}
