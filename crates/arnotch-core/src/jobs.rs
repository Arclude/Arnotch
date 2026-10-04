//! Downloads and file transfers in progress, as KDE Plasma shows them in its notifications:
//! browser downloads (through Plasma Browser Integration), copies and moves in Dolphin,
//! and other KDE apps' jobs.
//!
//! Apps report a job to Plasma's job view server by calling `update` (a map of changed
//! properties) and `terminate` on an `org.kde.JobViewV3` object. Those calls go to
//! plasmashell, not to everyone, so this watches the session bus as a monitor (as
//! `dbus-monitor` does) and keeps the state of each job.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zbus::blocking::fdo::MonitoringProxy;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{MatchRule, Message};

/// A transfer in progress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    /// Stable while the job runs.
    pub id: String,
    /// What the app calls it, such as "Downloading" or "Copying", in the user's language.
    pub title: String,
    /// The file (or folder) being written, by name.
    pub file: String,
    /// From 0 to 100.
    pub percent: u32,
    /// Bytes per second, 0 when unknown.
    pub speed: u64,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    /// Paused by the user.
    pub suspended: bool,
}

/// A job's properties as received, merged update after update.
#[derive(Default)]
struct Tracked {
    sender: String,
    props: HashMap<String, OwnedValue>,
}

impl Tracked {
    fn value(&self, key: &str) -> Option<&Value<'static>> {
        self.props.get(key).map(|v| &**v)
    }

    fn string(&self, key: &str) -> String {
        match self.value(key) {
            Some(Value::Str(text)) => text.to_string(),
            _ => String::new(),
        }
    }

    fn number(&self, key: &str) -> u64 {
        match self.value(key) {
            Some(Value::U64(n)) => *n,
            Some(Value::U32(n)) => u64::from(*n),
            Some(Value::I64(n)) => (*n).max(0) as u64,
            Some(Value::I32(n)) => (*n).max(0) as u64,
            _ => 0,
        }
    }

    fn job(&self, id: &str) -> Option<Job> {
        let total_bytes = self.number("totalBytes");
        let processed_bytes = self.number("processedBytes");
        let percent = match self.props.get("percent") {
            Some(_) => self.number("percent"),
            None if total_bytes > 0 => processed_bytes * 100 / total_bytes,
            None => return None, // nothing to show a bar for
        };
        Some(Job {
            id: id.to_owned(),
            title: self.string("title"),
            file: file_name(&self.string("destUrl"))
                .or_else(|| file_name(&self.string("descriptionValue2")))
                .or_else(|| file_name(&self.string("descriptionValue1")))
                .unwrap_or_default(),
            percent: percent.min(100) as u32,
            speed: self.number("speed"),
            processed_bytes,
            total_bytes,
            suspended: matches!(self.value("suspended"), Some(Value::Bool(true))),
        })
    }
}

/// The last part of a path or URL, decoded: "file:///home/me/a%20b.zip" gives "a b.zip".
fn file_name(location: &str) -> Option<String> {
    let location = location.trim().trim_end_matches('/');
    let name = location.rsplit('/').next()?.split(['?', '#']).next()?;
    (!name.is_empty()).then(|| percent_decode(name))
}

fn percent_decode(text: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push(hi << 4 | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Calls `on_change` with the transfers in progress whenever they change, at most a few
/// times a second. Runs as long as the session bus does, so call it on its own thread.
pub fn watch(on_change: impl Fn(Vec<Job>)) -> anyhow::Result<()> {
    let conn = Connection::session()?;
    let rules = [
        MatchRule::builder()
            .msg_type(Type::MethodCall)
            .interface("org.kde.JobViewV3")?
            .build(),
        // An app that quits (or crashes) takes its jobs with it.
        MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .build(),
    ];
    MonitoringProxy::new(&conn)?.become_monitor(&rules, 0)?;

    let mut jobs: HashMap<String, Tracked> = HashMap::new();
    let mut shown: Vec<Job> = Vec::new();
    let mut last_sent = Instant::now() - Duration::from_secs(1);
    for message in MessageIterator::from(&conn) {
        let message = message?;
        let mut ended = false;
        if !handle(&message, &mut jobs, &mut ended) {
            continue;
        }
        let mut now: Vec<Job> = jobs.iter().filter_map(|(id, job)| job.job(id)).collect();
        now.sort_by(|a, b| a.id.cmp(&b.id));
        // Progress is sent up to four times a second; a job starting or ending right away.
        let started = now.len() != shown.len();
        if now != shown && (ended || started || last_sent.elapsed() >= Duration::from_millis(250)) {
            on_change(now.clone());
            shown = now;
            last_sent = Instant::now();
        }
    }
    Ok(())
}

/// Applies one monitored message; true when the jobs changed.
fn handle(message: &Message, jobs: &mut HashMap<String, Tracked>, ended: &mut bool) -> bool {
    let header = message.header();
    let member = header.member().map(|m| m.as_str()).unwrap_or_default();
    if member == "NameOwnerChanged" {
        let Ok((name, _old, new)) = message.body().deserialize::<(String, String, String)>() else {
            return false;
        };
        if !new.is_empty() || !name.starts_with(':') {
            return false;
        }
        let before = jobs.len();
        jobs.retain(|_, job| job.sender != name);
        *ended = jobs.len() != before;
        return *ended;
    }
    let Some(path) = header.path() else {
        return false;
    };
    let id = path
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    match member {
        "update" => {
            let Ok(props) = message.body().deserialize::<HashMap<String, OwnedValue>>() else {
                return false;
            };
            let job = jobs.entry(id).or_default();
            job.sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
            job.props.extend(props);
            true
        }
        "terminate" => {
            *ended = jobs.remove(&id).is_some();
            *ended
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked(props: &[(&str, Value)]) -> Tracked {
        Tracked {
            sender: ":1.2".into(),
            props: props
                .iter()
                .map(|(k, v)| {
                    (
                        k.to_string(),
                        OwnedValue::try_from(v.try_clone().unwrap()).unwrap(),
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn names_the_file() {
        assert_eq!(
            file_name("file:///home/me/İndirilenler/a%20b.zip").as_deref(),
            Some("a b.zip")
        );
        assert_eq!(file_name("/tmp/folder/").as_deref(), Some("folder"));
        assert_eq!(
            file_name("https://x.org/f.iso?token=1").as_deref(),
            Some("f.iso")
        );
        assert_eq!(file_name("").as_deref(), None);
        assert_eq!(percent_decode("%C4%B0ndir%"), "İndir%");
        assert_eq!(percent_decode("%İ"), "%İ");
    }

    #[test]
    fn builds_a_job_from_its_properties() {
        let job = tracked(&[
            ("title", Value::from("Downloading")),
            ("destUrl", Value::from("file:///home/me/f.iso")),
            ("totalBytes", Value::from(1000u64)),
            ("processedBytes", Value::from(250u64)),
            ("speed", Value::from(100u64)),
        ])
        .job("JobView_3")
        .unwrap();
        assert_eq!(job.file, "f.iso");
        assert_eq!(job.percent, 25);
        assert_eq!(job.speed, 100);

        let job = tracked(&[("percent", Value::from(40u32))])
            .job("x")
            .unwrap();
        assert_eq!(job.percent, 40);
        // No size and no percentage: nothing to show.
        assert!(
            tracked(&[("title", Value::from("Deleting"))])
                .job("x")
                .is_none()
        );
    }
}
