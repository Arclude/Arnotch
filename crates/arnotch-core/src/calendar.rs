//! Upcoming meetings from a calendar's iCalendar (ICS) address, such as Google Calendar's
//! "secret address in iCal format". Recurring events are expanded with the `rrule` crate.

use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::str::FromStr;

use anyhow::{Context, bail};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rrule::RRuleSet;
use serde::{Deserialize, Serialize};

/// A timed event coming up (all-day events are left out).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Meeting {
    pub title: String,
    /// Unix seconds.
    pub start: i64,
    pub end: i64,
    /// A video call link (Meet, Zoom, Teams, ...) found in the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// Downloads the calendar. The address is a secret: it is never logged.
pub fn fetch(url: &str) -> anyhow::Result<String> {
    let url = url.trim().replacen("webcal://", "https://", 1);
    if !url.starts_with("https://") && !url.starts_with("http://") {
        bail!("the calendar address must start with https://");
    }
    let out = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--compressed",
        ])
        .args(["--max-time", "30", "--user-agent", "arnotch"])
        .arg(&url)
        .output()
        .context("cannot run curl")?;
    if !out.status.success() {
        // curl's message names the host, not the secret path.
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "download failed: {}",
            err.trim().replace(&url, "<calendar>")
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Timed events that end after `now` and start within `horizon`, soonest first.
pub fn upcoming(ics: &str, now: DateTime<Utc>, horizon: Duration) -> Vec<Meeting> {
    let events = parse_events(ics);
    // Instances moved or changed on their own (RECURRENCE-ID) replace the series' ones.
    let replaced: HashMap<&str, HashSet<i64>> = events
        .iter()
        .filter_map(|e| Some((e.uid.as_str(), e.recurrence_id?)))
        .fold(HashMap::new(), |mut map, (uid, at)| {
            map.entry(uid).or_default().insert(at.timestamp());
            map
        });

    let until = now + horizon;
    let mut meetings = Vec::new();
    for event in &events {
        if event.cancelled || event.all_day {
            continue;
        }
        let length = event
            .end
            .map_or(Duration::hours(1), |end| end - event.start);
        let starts = match (&event.rrule, event.recurrence_id) {
            (Some(rule), None) => {
                let skip = replaced.get(event.uid.as_str());
                expand(event, rule, now - length, until)
                    .into_iter()
                    .filter(|start| skip.is_none_or(|s| !s.contains(&start.timestamp())))
                    .collect()
            }
            _ => vec![event.start],
        };
        for start in starts {
            let end = start + length;
            if end > now && start < until {
                meetings.push(Meeting {
                    title: event.summary.clone(),
                    start: start.timestamp(),
                    end: end.timestamp(),
                    link: event.link.clone(),
                });
            }
        }
    }
    meetings.sort_by_key(|m| m.start);
    meetings.dedup();
    meetings
}

struct Event {
    uid: String,
    summary: String,
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
    all_day: bool,
    cancelled: bool,
    /// DTSTART and EXDATE lines as written, for the recurrence rule.
    dtstart_line: String,
    exdate_lines: Vec<String>,
    rrule: Option<String>,
    recurrence_id: Option<DateTime<Utc>>,
    link: Option<String>,
}

fn expand(
    event: &Event,
    rule: &str,
    from: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Vec<DateTime<Utc>> {
    let mut text = format!("{}\nRRULE:{rule}", event.dtstart_line);
    for exdate in &event.exdate_lines {
        text.push('\n');
        text.push_str(exdate);
    }
    let set = RRuleSet::from_str(&text).or_else(|_| {
        // A time zone chrono-tz doesn't know (Outlook writes Windows names): use UTC.
        let utc = format!(
            "DTSTART:{}\nRRULE:{rule}",
            event.start.format("%Y%m%dT%H%M%SZ")
        );
        RRuleSet::from_str(&utc)
    });
    let Ok(set) = set else {
        return vec![event.start];
    };
    set.after(from.with_timezone(&rrule::Tz::UTC))
        .before(until.with_timezone(&rrule::Tz::UTC))
        .all(200)
        .dates
        .into_iter()
        .map(|d| d.with_timezone(&Utc))
        .collect()
}

fn parse_events(ics: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut props: Vec<(String, String, String)> = Vec::new();
    let mut depth = 0; // components nested in the event (VALARM)
    let mut in_event = false;
    for line in unfold(ics) {
        let Some((name, params, value)) = split_property(&line) else {
            continue;
        };
        match (name.as_str(), value.as_str()) {
            ("BEGIN", "VEVENT") => {
                in_event = true;
                depth = 0;
                props.clear();
            }
            ("END", "VEVENT") => {
                in_event = false;
                if let Some(event) = build_event(&props) {
                    events.push(event);
                }
            }
            ("BEGIN", _) if in_event => depth += 1,
            ("END", _) if in_event => depth -= 1,
            _ if in_event && depth == 0 => props.push((name, params, value)),
            _ => {}
        }
    }
    events
}

fn build_event(props: &[(String, String, String)]) -> Option<Event> {
    let get = |key: &str| props.iter().find(|(name, _, _)| name == key);
    let (_, params, value) = get("DTSTART")?;
    let (start, all_day) = parse_time(params, value)?;
    let end = get("DTEND")
        .and_then(|(_, p, v)| parse_time(p, v))
        .map(|(t, _)| t)
        .or_else(|| {
            get("DURATION")
                .and_then(|(_, _, v)| parse_duration(v))
                .map(|d| start + d)
        });
    let text = |key: &str| get(key).map(|(_, _, v)| unescape(v)).unwrap_or_default();
    let link = ["LOCATION", "URL", "X-GOOGLE-CONFERENCE", "DESCRIPTION"]
        .iter()
        .find_map(|key| meeting_link(&text(key)));
    Some(Event {
        uid: text("UID"),
        summary: Some(text("SUMMARY"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "(no title)".into()),
        start,
        end,
        all_day,
        cancelled: text("STATUS").eq_ignore_ascii_case("CANCELLED"),
        dtstart_line: format!("DTSTART{params}:{value}"),
        exdate_lines: props
            .iter()
            .filter(|(name, _, _)| name == "EXDATE")
            .map(|(_, p, v)| format!("EXDATE{p}:{v}"))
            .collect(),
        rrule: get("RRULE").map(|(_, _, v)| v.clone()),
        recurrence_id: get("RECURRENCE-ID")
            .and_then(|(_, p, v)| parse_time(p, v))
            .map(|(t, _)| t),
        link,
    })
}

/// Joins folded lines (a continuation starts with a space or a tab).
fn unfold(ics: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in ics.lines() {
        let raw = raw.trim_end_matches('\r');
        if let Some(rest) = raw.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
            }
        } else if !raw.is_empty() {
            lines.push(raw.to_owned());
        }
    }
    lines
}

/// `NAME;PARAM=x:VALUE` into ("NAME", ";PARAM=x", "VALUE"). A colon inside a quoted
/// parameter value doesn't end the parameters.
fn split_property(line: &str) -> Option<(String, String, String)> {
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(i, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        (c == ':' && !quoted).then_some(i)
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let (name, params) = head.split_at(head.find(';').unwrap_or(head.len()));
    Some((
        name.to_ascii_uppercase(),
        params.to_owned(),
        value.to_owned(),
    ))
}

/// A DTSTART-like value: UTC (`...Z`), in a TZID, floating (local time) or a date.
fn parse_time(params: &str, value: &str) -> Option<(DateTime<Utc>, bool)> {
    let value = value.trim();
    if value.len() == 8 {
        let date = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
        return Some((Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?), true));
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some((Utc.from_utc_datetime(&naive), false));
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    let tzid = params
        .split(';')
        .find_map(|p| p.strip_prefix("TZID="))
        .map(|tz| tz.trim_matches('"'));
    let time = match tzid.and_then(|tz| chrono_tz::Tz::from_str(tz).ok()) {
        Some(tz) => tz
            .from_local_datetime(&naive)
            .earliest()?
            .with_timezone(&Utc),
        None => chrono::Local
            .from_local_datetime(&naive)
            .earliest()?
            .with_timezone(&Utc),
    };
    Some((time, false))
}

/// `PT1H30M`, `P1D`, ...
fn parse_duration(value: &str) -> Option<Duration> {
    let rest = value.trim().strip_prefix('P')?;
    let (mut total, mut number) = (0i64, String::new());
    for c in rest.chars() {
        match c {
            '0'..='9' => number.push(c),
            'T' => {}
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                total += n * match c {
                    'W' => 604_800,
                    'D' => 86_400,
                    'H' => 3_600,
                    'M' => 60,
                    _ => 1,
                };
            }
            _ => return None,
        }
    }
    Some(Duration::seconds(total))
}

fn unescape(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// The first video call link in the text.
fn meeting_link(text: &str) -> Option<String> {
    const HOSTS: &[&str] = &[
        "meet.google.com",
        "zoom.us",
        "teams.microsoft.com",
        "teams.live.com",
        "webex.com",
        "whereby.com",
        "meet.jit.si",
    ];
    text.match_indices("https://").find_map(|(at, _)| {
        let url: String = text[at..]
            .chars()
            .take_while(|c| !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '\'' | ')' | ']'))
            .collect();
        let host = url["https://".len()..].split(['/', '?']).next()?;
        HOSTS
            .iter()
            .any(|h| host == *h || host.ends_with(&format!(".{h}")))
            .then_some(url)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r
BEGIN:VEVENT\r
UID:standup@x\r
SUMMARY:Daily standup\r
DTSTART;TZID=Europe/Istanbul:20261001T100000\r
DTEND;TZID=Europe/Istanbul:20261001T101500\r
RRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR\r
EXDATE;TZID=Europe/Istanbul:20261006T100000\r
LOCATION:https://meet.google.com/abc-defg-hij\r
BEGIN:VALARM\r
TRIGGER:-PT10M\r
DESCRIPTION:not the event's\r
END:VALARM\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:standup@x\r
RECURRENCE-ID;TZID=Europe/Istanbul:20261007T100000\r
SUMMARY:Daily standup (moved)\r
DTSTART;TZID=Europe/Istanbul:20261007T113000\r
DTEND;TZID=Europe/Istanbul:20261007T114500\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:review@x\r
SUMMARY:Design review\\, Q4\r
DTSTART:20261005T120000Z\r
DURATION:PT1H\r
DESCRIPTION:Join: https://us02web.zoom.us/j/123?pwd=x\\nThanks\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:holiday@x\r
SUMMARY:Holiday\r
DTSTART;VALUE=DATE:20261005\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:cancelled@x\r
SUMMARY:Cancelled one\r
STATUS:CANCELLED\r
DTSTART:20261005T150000Z\r
END:VEVENT\r
END:VCALENDAR\r
";

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn lists_the_days_meetings() {
        // Monday 5 October 2026, 08:00 in Istanbul (UTC+3).
        let meetings = upcoming(ICS, at("2026-10-05T05:00:00Z"), Duration::hours(24));
        let titles: Vec<&str> = meetings.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["Daily standup", "Design review, Q4"]);
        assert_eq!(meetings[0].start, at("2026-10-05T07:00:00Z").timestamp());
        assert_eq!(meetings[0].end - meetings[0].start, 15 * 60);
        assert_eq!(
            meetings[0].link.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(
            meetings[1].link.as_deref(),
            Some("https://us02web.zoom.us/j/123?pwd=x")
        );
        assert_eq!(meetings[1].end - meetings[1].start, 3600);
    }

    #[test]
    fn honours_exdate_and_moved_instances() {
        // Tuesday 6 Oct is excluded; Wednesday 7 Oct moved to 11:30.
        let tuesday = upcoming(ICS, at("2026-10-06T05:00:00Z"), Duration::hours(12));
        assert!(tuesday.is_empty(), "{tuesday:?}");
        let wednesday = upcoming(ICS, at("2026-10-07T05:00:00Z"), Duration::hours(12));
        assert_eq!(wednesday.len(), 1);
        assert_eq!(wednesday[0].title, "Daily standup (moved)");
        assert_eq!(wednesday[0].start, at("2026-10-07T08:30:00Z").timestamp());
    }

    #[test]
    fn keeps_a_meeting_that_is_under_way() {
        let meetings = upcoming(ICS, at("2026-10-05T07:10:00Z"), Duration::hours(1));
        assert_eq!(
            meetings.first().map(|m| m.title.as_str()),
            Some("Daily standup")
        );
    }

    #[test]
    fn parses_small_pieces() {
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(parse_duration("P1DT2H"), Some(Duration::hours(26)));
        assert_eq!(
            split_property("DTSTART;TZID=\"Europe/Istanbul\":20261001T100000"),
            Some((
                "DTSTART".into(),
                ";TZID=\"Europe/Istanbul\"".into(),
                "20261001T100000".into()
            ))
        );
        assert_eq!(
            meeting_link("see https://example.com and https://teams.microsoft.com/l/x)"),
            Some("https://teams.microsoft.com/l/x".into())
        );
    }
}
