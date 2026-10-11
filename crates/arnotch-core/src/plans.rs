//! How much of their plans the agents other than Claude Code have used. Claude Code's
//! limits come with its status line ([`crate::claude::Usage`]); these are read from the
//! agents' own files.
//!
//! - Codex writes its rate limits (5 hours, week) into each session's rollout file under
//!   `~/.codex/sessions/YYYY/MM/DD/`.
//! - Hermes keeps its token counts per session in `~/.hermes/state.db` (SQLite, read with
//!   the `sqlite3` program). Its providers have no plan limits to read, so the island shows
//!   the tokens used today and in the last 7 days.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An agent's plan, for the island.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// "codex", "hermes".
    pub agent: String,
    /// The plan's kind ("plus", "pro") or the model in use ("glm-5.2 · zai").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Rate limit windows, the shortest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<Window>,
    /// Without limits to show: the tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Tokens>,
    /// When the numbers were written by the agent, Unix seconds.
    pub updated: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// How long the window is, in minutes (300 for 5 hours, 10080 for a week).
    pub minutes: u32,
    /// 0 to 100.
    pub used_percent: f64,
    /// Unix seconds.
    pub resets_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub today: u64,
    pub week: u64,
    pub cost_today_usd: f64,
    pub cost_week_usd: f64,
}

/// The plans that can be read now.
pub fn read() -> Vec<Plan> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut plans = Vec::new();
    if let Some(plan) = codex(&home.join(".codex/sessions")) {
        plans.push(plan);
    }
    if let Some(plan) = hermes(&home.join(".hermes/state.db")) {
        plans.push(plan);
    }
    plans
}

/// Codex's latest rate limits: the last `rate_limits` in its newest session file.
fn codex(sessions: &Path) -> Option<Plan> {
    let file = newest_rollout(sessions)?;
    let text = std::fs::read_to_string(&file).ok()?;
    let updated = std::fs::metadata(&file)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs() as i64);
    text.lines()
        .rev()
        .find_map(|line| codex_limits(line, updated))
}

/// The newest `rollout-*.jsonl` under `sessions/YYYY/MM/DD/`: the last folder at each level.
fn newest_rollout(sessions: &Path) -> Option<PathBuf> {
    let last = |dir: &Path, files: bool| -> Option<PathBuf> {
        std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                if files {
                    path.extension().is_some_and(|ext| ext == "jsonl")
                } else {
                    path.is_dir()
                }
            })
            .max()
    };
    let year = last(sessions, false)?;
    let month = last(&year, false)?;
    let day = last(&month, false)?;
    last(&day, true)
}

/// The rate limits in a rollout line, if it has any: `{"type":"event_msg","payload":
/// {"type":"token_count","rate_limits":{"primary":{...},"secondary":{...}}}}`.
fn codex_limits(line: &str, updated: i64) -> Option<Plan> {
    let value: Value = serde_json::from_str(line).ok()?;
    let limits = value
        .pointer("/payload/rate_limits")
        .or_else(|| value.get("rate_limits"))
        .filter(|limits| limits.is_object())?;
    // When the line was written: resets given in seconds from then count from it.
    let written = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
        .map_or(updated, |time| time.timestamp());
    let window = |key: &str| -> Option<Window> {
        let window = limits.get(key)?;
        let number = |key: &str| window.get(key).and_then(Value::as_f64);
        Some(Window {
            minutes: number("window_minutes").unwrap_or(0.0) as u32,
            used_percent: number("used_percent")?,
            resets_at: number("resets_at")
                .map(|at| at as i64)
                .or_else(|| number("resets_in_seconds").map(|secs| written + secs as i64))
                .unwrap_or(0),
        })
    };
    let windows: Vec<Window> = ["primary", "secondary"]
        .iter()
        .filter_map(|key| window(key))
        .collect();
    if windows.is_empty() {
        return None;
    }
    Some(Plan {
        agent: "codex".into(),
        name: limits
            .get("plan_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        windows,
        tokens: None,
        updated: written,
    })
}

/// Hermes' tokens today and in the last 7 days, with the model it used last.
fn hermes(db: &Path) -> Option<Plan> {
    if !db.is_file() {
        return None;
    }
    let now = chrono::Local::now();
    let midnight = now
        .date_naive()
        .and_hms_opt(0, 0, 0)?
        .and_local_timezone(chrono::Local)
        .earliest()?
        .timestamp();
    let week = now.timestamp() - 7 * 24 * 3600;
    let sql = format!(
        "select \
           coalesce(sum(case when started_at >= {midnight} then input_tokens + output_tokens end), 0) as today, \
           coalesce(sum(input_tokens + output_tokens), 0) as week, \
           coalesce(sum(case when started_at >= {midnight} then coalesce(actual_cost_usd, estimated_cost_usd) end), 0) as cost_today, \
           coalesce(sum(coalesce(actual_cost_usd, estimated_cost_usd)), 0) as cost_week, \
           (select model from sessions order by started_at desc limit 1) as model, \
           (select billing_provider from sessions order by started_at desc limit 1) as provider, \
           (select max(coalesce(last_activity_at, started_at)) from sessions) as updated \
         from sessions where started_at >= {week}"
    );
    let output = Command::new("sqlite3")
        .args(["-readonly", "-json"])
        .arg(db)
        .arg(sql)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    hermes_row(&String::from_utf8_lossy(&output.stdout))
}

/// The row `sqlite3 -json` printed for the query in [`hermes`].
fn hermes_row(json: &str) -> Option<Plan> {
    let rows: Value = serde_json::from_str(json).ok()?;
    let row = rows.get(0)?;
    let number = |key: &str| row.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let text = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default();
    let name = match (text("model"), text("provider")) {
        ("", _) => String::new(),
        (model, "") => model.to_owned(),
        (model, provider) => format!("{model} · {provider}"),
    };
    Some(Plan {
        agent: "hermes".into(),
        name,
        windows: Vec::new(),
        tokens: Some(Tokens {
            today: number("today") as u64,
            week: number("week") as u64,
            cost_today_usd: number("cost_today"),
            cost_week_usd: number("cost_week"),
        }),
        updated: number("updated") as i64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_codex_rate_limits() {
        let line = r#"{"timestamp":"2026-10-10T18:00:00.000Z","type":"event_msg","payload":{"type":"token_count",
            "info":{"total_token_usage":{"input_tokens":10}},"rate_limits":{"plan_type":"plus",
            "primary":{"used_percent":22.0,"window_minutes":300,"resets_at":1791140700},
            "secondary":{"used_percent":47.0,"window_minutes":10080,"resets_in_seconds":3600}}}}"#;
        let plan = codex_limits(line, 0).unwrap();
        assert_eq!(plan.name, "plus");
        assert_eq!(plan.windows.len(), 2);
        assert_eq!(
            plan.windows[0],
            Window {
                minutes: 300,
                used_percent: 22.0,
                resets_at: 1791140700
            }
        );
        // 2026-10-10T18:00:00Z is 1791655200; reset an hour later.
        assert_eq!(plan.windows[1].resets_at, 1791655200 + 3600);
        assert_eq!(
            codex_limits(
                r#"{"type":"event_msg","payload":{"type":"agent_message"}}"#,
                0
            ),
            None
        );
    }

    #[test]
    fn finds_the_newest_rollout() {
        let dir = std::env::temp_dir().join(format!("arnotch-plans-{}", std::process::id()));
        for day in ["2026/09/30", "2026/10/02", "2026/10/10"] {
            std::fs::create_dir_all(dir.join(day)).unwrap();
        }
        std::fs::write(dir.join("2026/10/10/rollout-a.jsonl"), "").unwrap();
        std::fs::write(dir.join("2026/10/10/rollout-b.jsonl"), "").unwrap();
        assert_eq!(
            newest_rollout(&dir),
            Some(dir.join("2026/10/10/rollout-b.jsonl"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_hermes_tokens() {
        let plan = hermes_row(
            r#"[{"today":21382,"week":40000,"cost_today":0.0,"cost_week":0.12,"model":"glm-5.2","provider":"zai","updated":1786282004.5}]"#,
        )
        .unwrap();
        assert_eq!(plan.name, "glm-5.2 · zai");
        assert_eq!(plan.tokens.unwrap().today, 21382);
        assert_eq!(plan.updated, 1786282004);
        assert_eq!(hermes_row("[]"), None);
    }
}
