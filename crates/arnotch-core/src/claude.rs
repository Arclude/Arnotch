//! Converts Claude Code hook payloads (read from the hook's stdin) into [`Event`]s.
//!
//! See <https://code.claude.com/docs/en/hooks> for the payload format.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diff;
use crate::event::{AgentPhase, Decision, Diff, Event, Reply};

/// How many lines of command output the island shows.
const OUTPUT_LINES: usize = 6;

#[derive(Debug, Deserialize)]
struct HookInput {
    session_id: String,
    #[serde(default)]
    cwd: String,
    hook_event_name: String,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: Value,
    #[serde(default)]
    tool_response: Value,
    #[serde(default)]
    error: Value,
    #[serde(default)]
    message: Option<String>,
    /// For Notification: "permission_prompt", "idle_prompt", "auth_success",
    /// "elicitation_dialog", ... (absent in older Claude Code versions).
    #[serde(default)]
    notification_type: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    tool_use_id: Option<String>,
    /// Set when a subagent calls the tool.
    #[serde(default)]
    agent_id: Option<String>,
}

/// Returns `None` for hook events the notch does not care about.
pub fn event_from_hook(json: &str) -> anyhow::Result<Option<Event>> {
    let input: HookInput = serde_json::from_str(json)?;
    let tool = input.tool_name.clone().unwrap_or_default();

    let phase = match input.hook_event_name.as_str() {
        "UserPromptSubmit" => AgentPhase::Started {
            prompt: first_line(input.prompt.as_deref().unwrap_or_default(), 120),
        },
        "PreToolUse" => AgentPhase::Tool {
            detail: tool_detail(&tool, &input.tool_input),
            diff: file_diff(&tool, &input.tool_input),
            tool,
        },
        "PostToolUse" => AgentPhase::ToolResult {
            ok: !response_failed(&input.tool_response),
            output: if tool == "Bash" {
                command_output(&input.tool_response)
            } else {
                Vec::new()
            },
            tool,
        },
        "PostToolUseFailure" => AgentPhase::ToolResult {
            ok: false,
            output: tail(&text_of(&input.error)),
            tool,
        },
        "PermissionRequest" => AgentPhase::Permission {
            request_id: input
                .tool_use_id
                .clone()
                .unwrap_or_else(|| format!("{}-{}", input.session_id, std::process::id())),
            detail: tool_detail(&tool, &input.tool_input),
            diff: file_diff(&tool, &input.tool_input),
            plan: (tool == "ExitPlanMode").then(|| str_field(&input.tool_input, "plan").to_owned()),
            questions: (tool == "AskUserQuestion")
                .then(|| input.tool_input.get("questions").cloned())
                .flatten(),
            tool,
        },
        // Only notifications that need an answer mean Claude waits for the user. The
        // idle reminder ("Claude is waiting for your input") comes after a finished
        // answer and must not turn a done session back into a waiting one.
        "Notification" => match input.notification_type.as_deref() {
            None | Some("permission_prompt" | "elicitation_dialog") => AgentPhase::Waiting {
                message: input.message.unwrap_or_default(),
            },
            Some(_) => return Ok(None),
        },
        "Stop" => AgentPhase::Done,
        "SessionEnd" => AgentPhase::Ended,
        _ => return Ok(None),
    };

    let project = Path::new(&input.cwd)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    Ok(Some(Event::Agent {
        session_id: input.session_id,
        project,
        cwd: input.cwd,
        tool_use_id: input.tool_use_id.unwrap_or_default(),
        agent_id: input.agent_id.unwrap_or_default(),
        phase,
    }))
}

/// What Claude Code's status line input says about a session and the account's limits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub session_id: String,
    /// Short project name, as in agent events.
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub model: String,
    /// Estimated session cost in US dollars (list price).
    #[serde(default)]
    pub cost_usd: f64,
    /// How full the context window is, 0 to 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_percent: Option<f64>,
    /// Pro and Max plans only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<Window>,
}

/// A rate limit window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// 0 to 100.
    pub used_percent: f64,
    /// Unix seconds.
    pub resets_at: i64,
}

/// Reads Claude Code's status line input (see its statusline documentation).
pub fn usage_from_statusline(json: &str) -> Option<Usage> {
    let input: Value = serde_json::from_str(json).ok()?;
    let number = |path: &[&str]| -> Option<f64> {
        path.iter()
            .try_fold(&input, |value, key| value.get(key))?
            .as_f64()
    };
    let window = |name: &str| -> Option<Window> {
        Some(Window {
            used_percent: number(&["rate_limits", name, "used_percentage"])?,
            resets_at: number(&["rate_limits", name, "resets_at"])? as i64,
        })
    };
    let cwd = input
        .pointer("/workspace/project_dir")
        .or_else(|| input.get("cwd"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(Usage {
        session_id: input.get("session_id")?.as_str()?.to_owned(),
        project: Path::new(cwd)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        model: input
            .pointer("/model/display_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        cost_usd: number(&["cost", "total_cost_usd"]).unwrap_or(0.0),
        context_percent: number(&["context_window", "used_percentage"]),
        five_hour: window("five_hour"),
        seven_day: window("seven_day"),
    })
}

/// Whether the tool waits for the user's own input (a plan to approve, questions to
/// answer) rather than a plain yes or no, so the island waits longer for it.
pub fn needs_input(hook_json: &str) -> bool {
    serde_json::from_str::<HookInput>(hook_json).is_ok_and(|input| {
        matches!(
            input.tool_name.as_deref(),
            Some("ExitPlanMode" | "AskUserQuestion")
        )
    })
}

/// What the hook prints so Claude Code applies the user's reply to the request in
/// `hook_json`. `None` lets Claude Code ask in the terminal as usual.
///
/// A plan approval and questions need more than "allow": the tool's input goes back
/// with the answers (`updatedInput`), and approving a plan sets the permission mode
/// to carry on in (`updatedPermissions`).
pub fn permission_output(reply: &Reply, hook_json: &str) -> Option<String> {
    let input: HookInput = serde_json::from_str(hook_json).ok()?;
    let tool = input.tool_name.as_deref().unwrap_or_default();
    let decision = match reply.decision {
        Decision::Allow if tool == "AskUserQuestion" => {
            let mut updated = input.tool_input.clone();
            updated
                .as_object_mut()?
                .insert("answers".into(), reply.answers.clone()?.into());
            serde_json::json!({ "behavior": "allow", "updatedInput": updated })
        }
        Decision::Allow if tool == "ExitPlanMode" => {
            let mut decision =
                serde_json::json!({ "behavior": "allow", "updatedInput": input.tool_input });
            if let Some(mode) = &reply.mode {
                decision["updatedPermissions"] = serde_json::json!([{ "type": "setMode", "mode": mode, "destination": "session" }]);
            }
            decision
        }
        Decision::Allow => serde_json::json!({ "behavior": "allow" }),
        Decision::Deny => serde_json::json!({
            "behavior": "deny",
            "message": reply
                .message
                .clone()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| crate::i18n::tr("Denied from arnotch").into()),
        }),
        Decision::Ask => return None,
    };
    Some(
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": decision,
            }
        })
        .to_string(),
    )
}

/// A one-line, human readable summary of what a tool call is about.
fn tool_detail(tool: &str, input: &Value) -> String {
    let field = |key: &str| str_field(input, key);

    let detail = match tool {
        "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => file_name(field("file_path")),
        "Bash" => field("command").to_owned(),
        "Grep" | "Glob" => field("pattern").to_owned(),
        "WebFetch" => field("url").to_owned(),
        "WebSearch" => field("query").to_owned(),
        "Task" | "Agent" => field("description").to_owned(),
        _ => String::new(),
    };

    first_line(&detail, 80)
}

/// What a file-changing tool call is going to change.
pub(crate) fn file_diff(tool: &str, input: &Value) -> Option<Diff> {
    let path = str_field(input, "file_path");
    let name = file_name(path);

    match tool {
        "Edit" => Some(edit_diff(&name, path, input)),
        // Only the first edit; the island has no room for more.
        "MultiEdit" => input
            .get("edits")?
            .as_array()?
            .first()
            .map(|edit| edit_diff(&name, path, edit)),
        "Write" => {
            let old = diff::read_small(path).unwrap_or_default();
            Some(diff::excerpt(&name, &old, str_field(input, "content"), 1))
        }
        _ => None,
    }
}

fn edit_diff(name: &str, path: &str, edit: &Value) -> Diff {
    let old = str_field(edit, "old_string");
    let new = str_field(edit, "new_string");
    // The hook runs before the edit, so the file still contains `old`.
    let first_line = diff::read_small(path).map_or(0, |text| diff::line_of(&text, old));
    diff::excerpt(name, old, new, first_line)
}

fn response_failed(response: &Value) -> bool {
    let flag = |key: &str| response.get(key).and_then(Value::as_bool) == Some(true);
    flag("interrupted") || flag("is_error")
}

/// The last lines of a command's output, stderr included.
fn command_output(response: &Value) -> Vec<String> {
    let text = match response {
        Value::Object(_) => {
            let stdout = str_field(response, "stdout");
            let stderr = str_field(response, "stderr");
            format!("{stdout}\n{stderr}")
        }
        other => text_of(other),
    };
    tail(&text)
}

fn tail(text: &str) -> Vec<String> {
    let clean = strip_ansi(text);
    let mut lines: Vec<String> = clean
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .rev()
        .take(OUTPUT_LINES)
        .map(|line| first_line(line, 160))
        .collect();
    lines.reverse();
    lines
}

fn text_of(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Removes terminal escape sequences (colors, cursor moves) from command output.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // CSI sequence: parameters, then one final byte in '@'..='~'
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        } else {
            chars.next();
        }
    }
    out
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or(path.to_owned(), |name| name.to_string_lossy().into_owned())
}

fn first_line(text: &str, max_chars: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    match line.char_indices().nth(max_chars) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{DiffLine, LineKind};

    fn phase(json: &str) -> AgentPhase {
        match event_from_hook(json).unwrap().unwrap() {
            Event::Agent { phase, .. } => phase,
            other => panic!("unexpected event {other:?}"),
        }
    }

    #[test]
    fn pre_tool_use_becomes_tool_phase() {
        let json = r#"{
            "session_id": "abc",
            "cwd": "/home/me/projects/korus",
            "hook_event_name": "PreToolUse",
            "tool_name": "Read",
            "tool_input": { "file_path": "/home/me/projects/korus/src/invoice.ts" }
        }"#;

        let event = event_from_hook(json).unwrap().unwrap();
        assert_eq!(
            event,
            Event::Agent {
                session_id: "abc".into(),
                project: "korus".into(),
                cwd: "/home/me/projects/korus".into(),
                tool_use_id: String::new(),
                agent_id: String::new(),
                phase: AgentPhase::Tool {
                    tool: "Read".into(),
                    detail: "invoice.ts".into(),
                    diff: None,
                },
            }
        );
    }

    #[test]
    fn edits_carry_a_diff_with_real_line_numbers() {
        let dir = std::env::temp_dir().join(format!("notch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("invoice.ts");
        std::fs::write(&file, "import x;\n\nconst TAX = 0.2;\n").unwrap();

        let json = serde_json::json!({
            "session_id": "abc",
            "cwd": "/p/korus",
            "hook_event_name": "PreToolUse",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": file,
                "old_string": "const TAX = 0.2;",
                "new_string": "const TAX = 0.18;"
            }
        });
        let AgentPhase::Tool {
            diff: Some(diff), ..
        } = phase(&json.to_string())
        else {
            panic!("expected a diff");
        };
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(diff.file, "invoice.ts");
        assert_eq!(
            diff.lines,
            [
                DiffLine {
                    kind: LineKind::Del,
                    n: 3,
                    text: "const TAX = 0.2;".into()
                },
                DiffLine {
                    kind: LineKind::Add,
                    n: 3,
                    text: "const TAX = 0.18;".into()
                },
            ]
        );
    }

    #[test]
    fn bash_results_keep_the_clean_output_tail() {
        let json = r#"{
            "session_id": "abc",
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_response": {
                "stdout": "a\nb\nc\nd\ne\n\u001b[32mPASS\u001b[0m src/invoice.test.ts\nTests: 48 passed, 48 total\n",
                "stderr": "",
                "interrupted": false
            }
        }"#;
        assert_eq!(
            phase(json),
            AgentPhase::ToolResult {
                tool: "Bash".into(),
                ok: true,
                output: vec![
                    "b".into(),
                    "c".into(),
                    "d".into(),
                    "e".into(),
                    "PASS src/invoice.test.ts".into(),
                    "Tests: 48 passed, 48 total".into(),
                ],
            }
        );
    }

    #[test]
    fn failures_are_reported() {
        let json = r#"{
            "session_id": "abc",
            "hook_event_name": "PostToolUseFailure",
            "tool_name": "Bash",
            "error": "Command failed with exit code 1"
        }"#;
        assert_eq!(
            phase(json),
            AgentPhase::ToolResult {
                tool: "Bash".into(),
                ok: false,
                output: vec!["Command failed with exit code 1".into()],
            }
        );
    }

    #[test]
    fn permission_requests_carry_the_tool_use_id() {
        let json = r#"{
            "session_id": "abc",
            "cwd": "/p/korus",
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": { "command": "rm -rf /tmp/build" },
            "tool_use_id": "toolu_01"
        }"#;
        assert_eq!(
            phase(json),
            AgentPhase::Permission {
                request_id: "toolu_01".into(),
                tool: "Bash".into(),
                detail: "rm -rf /tmp/build".into(),
                diff: None,
                plan: None,
                questions: None,
            }
        );
    }

    const BASH_REQUEST: &str = r#"{"session_id":"abc","cwd":"/x","hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"ls"}}"#;

    fn output(reply: Reply, request: &str) -> Value {
        serde_json::from_str(&permission_output(&reply, request).unwrap()).unwrap()
    }

    #[test]
    fn decisions_become_hook_output() {
        let allow = output(Decision::Allow.into(), BASH_REQUEST);
        assert_eq!(
            allow["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(allow["hookSpecificOutput"]["decision"]["behavior"], "allow");
        assert_eq!(permission_output(&Decision::Ask.into(), BASH_REQUEST), None);

        let deny = output(
            Reply {
                decision: Decision::Deny,
                message: Some("not now".into()),
                ..Default::default()
            },
            BASH_REQUEST,
        );
        assert_eq!(deny["hookSpecificOutput"]["decision"]["message"], "not now");
    }

    #[test]
    fn answers_questions_with_the_tool_input() {
        let request = r#"{"session_id":"abc","cwd":"/x","hook_event_name":"PermissionRequest","tool_name":"AskUserQuestion",
            "tool_input":{"questions":[{"question":"Which framework?","header":"Framework","options":[{"label":"React"},{"label":"Vue"}],"multiSelect":false}]}}"#;
        assert!(needs_input(request));
        let Some(Event::Agent {
            phase: AgentPhase::Permission { questions, .. },
            ..
        }) = event_from_hook(request).unwrap()
        else {
            panic!("not a permission request");
        };
        assert_eq!(questions.unwrap()[0]["header"], "Framework");

        let mut answers = serde_json::Map::new();
        answers.insert("Which framework?".into(), "Vue".into());
        let reply = Reply {
            decision: Decision::Allow,
            answers: Some(answers),
            ..Default::default()
        };
        let decision = &output(reply, request)["hookSpecificOutput"]["decision"];
        assert_eq!(decision["behavior"], "allow");
        assert_eq!(
            decision["updatedInput"]["answers"]["Which framework?"],
            "Vue"
        );
        assert_eq!(
            decision["updatedInput"]["questions"][0]["question"],
            "Which framework?"
        );

        // Allowing without answers can't satisfy the tool: ask in the terminal instead.
        assert_eq!(permission_output(&Decision::Allow.into(), request), None);
    }

    #[test]
    fn approves_plans_in_a_permission_mode() {
        let request = r#"{"session_id":"abc","cwd":"/x","hook_event_name":"PermissionRequest","tool_name":"ExitPlanMode",
            "tool_input":{"plan":"Plan\n1. Do it","planFilePath":"/p.md"}}"#;
        let Some(Event::Agent {
            phase: AgentPhase::Permission { plan, .. },
            ..
        }) = event_from_hook(request).unwrap()
        else {
            panic!("not a permission request");
        };
        assert_eq!(plan.as_deref(), Some("Plan\n1. Do it"));

        let reply = Reply {
            decision: Decision::Allow,
            mode: Some("acceptEdits".into()),
            ..Default::default()
        };
        let decision = &output(reply, request)["hookSpecificOutput"]["decision"];
        assert_eq!(decision["updatedInput"]["planFilePath"], "/p.md");
        assert_eq!(decision["updatedPermissions"][0]["type"], "setMode");
        assert_eq!(decision["updatedPermissions"][0]["mode"], "acceptEdits");
        assert_eq!(decision["updatedPermissions"][0]["destination"], "session");

        let keep = Reply {
            decision: Decision::Deny,
            message: Some("Add tests".into()),
            ..Default::default()
        };
        assert_eq!(
            output(keep, request)["hookSpecificOutput"]["decision"]["message"],
            "Add tests"
        );
    }

    #[test]
    fn unknown_hooks_are_ignored() {
        let json = r#"{"session_id": "abc", "hook_event_name": "PreCompact"}"#;
        assert_eq!(event_from_hook(json).unwrap(), None);
    }

    #[test]
    fn long_commands_are_truncated() {
        let long = "x".repeat(200);
        let detail = tool_detail("Bash", &serde_json::json!({ "command": long }));
        assert_eq!(detail.chars().count(), 81);
    }

    #[test]
    fn events_serialize_flat() {
        let event = Event::Agent {
            session_id: "abc".into(),
            project: "korus".into(),
            cwd: String::new(),
            tool_use_id: String::new(),
            agent_id: String::new(),
            phase: AgentPhase::Done,
        };
        assert_eq!(
            event.to_line(),
            "{\"type\":\"agent\",\"session_id\":\"abc\",\"project\":\"korus\",\"phase\":\"done\"}\n"
        );
    }

    #[test]
    fn only_notifications_that_need_an_answer_mean_waiting() {
        let notification = |kind: &str| {
            format!(
                r#"{{"session_id":"abc","cwd":"/x/korus","hook_event_name":"Notification","message":"m"{kind}}}"#
            )
        };
        let waiting = |json: &str| {
            matches!(
                event_from_hook(json).unwrap(),
                Some(Event::Agent {
                    phase: AgentPhase::Waiting { .. },
                    ..
                })
            )
        };
        assert!(waiting(&notification(
            r#","notification_type":"permission_prompt""#
        )));
        assert!(waiting(&notification(
            r#","notification_type":"elicitation_dialog""#
        )));
        // Older Claude Code versions send no type.
        assert!(waiting(&notification("")));
        for ignored in ["idle_prompt", "auth_success"] {
            let json = notification(&format!(r#","notification_type":"{ignored}""#));
            assert_eq!(event_from_hook(&json).unwrap(), None, "{ignored}");
        }
    }

    #[test]
    fn reads_the_status_line_input() {
        let json = r#"{"session_id":"s1","cwd":"/x/korus","workspace":{"project_dir":"/x/korus"},
            "model":{"id":"claude-opus-5-5","display_name":"Opus 5.5"},
            "cost":{"total_cost_usd":1.25},"context_window":{"used_percentage":34.5},
            "rate_limits":{"five_hour":{"used_percentage":57,"resets_at":1791084000},
                           "seven_day":{"used_percentage":43.5,"resets_at":1791414000}}}"#;
        let usage = usage_from_statusline(json).unwrap();
        assert_eq!(usage.project, "korus");
        assert_eq!(usage.model, "Opus 5.5");
        assert_eq!(usage.cost_usd, 1.25);
        assert_eq!(usage.context_percent, Some(34.5));
        assert_eq!(
            usage.five_hour,
            Some(Window {
                used_percent: 57.0,
                resets_at: 1791084000
            })
        );
        assert_eq!(usage.seven_day.unwrap().used_percent, 43.5);

        // API key users get no rate limits.
        let usage =
            usage_from_statusline(r#"{"session_id":"s2","cost":{"total_cost_usd":0.1}}"#).unwrap();
        assert_eq!(usage.five_hour, None);
        assert_eq!(usage_from_statusline("not json"), None);
    }
}
