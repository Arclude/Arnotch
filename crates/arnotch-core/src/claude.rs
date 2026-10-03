//! Converts Claude Code hook payloads (read from the hook's stdin) into [`Event`]s.
//!
//! See <https://code.claude.com/docs/en/hooks> for the payload format.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::diff;
use crate::event::{AgentPhase, Decision, Diff, Event};

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
        phase,
    }))
}

/// What the hook prints so Claude Code applies the user's decision.
/// `None` lets Claude Code ask in the terminal as usual.
pub fn permission_output(decision: Decision) -> Option<String> {
    let decision = match decision {
        Decision::Allow => serde_json::json!({ "behavior": "allow" }),
        Decision::Deny => serde_json::json!({
            "behavior": "deny",
            "message": crate::i18n::tr("Denied from arnotch"),
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
fn file_diff(tool: &str, input: &Value) -> Option<Diff> {
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
            }
        );
    }

    #[test]
    fn decisions_become_hook_output() {
        let allow: Value =
            serde_json::from_str(&permission_output(Decision::Allow).unwrap()).unwrap();
        assert_eq!(
            allow["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(allow["hookSpecificOutput"]["decision"]["behavior"], "allow");
        assert_eq!(permission_output(Decision::Ask), None);
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
}
