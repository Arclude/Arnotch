//! Coding agents other than Claude Code: their hook payloads become the same [`Event`]s as
//! Claude Code's, and a permission answered in the island goes back in each one's format.
//!
//! Each agent calls `arnotchctl agent-hook <name>` with its hook payload on stdin. The
//! payload is turned into Claude Code's shape (event names, tool names and fields), which
//! [`claude::event_from_hook`] already understands; [`connect`] adds the hooks to the
//! agent's own configuration.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::claude;
use crate::event::{Decision, Event, Reply};

/// The agents arnotch knows, by the name used in `arnotchctl agent-hook <name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
    Gemini,
    Antigravity,
    Cursor,
    Copilot,
    OpenCode,
    Amp,
    Hermes,
}

pub const ALL: [Agent; 9] = [
    Agent::Claude,
    Agent::Codex,
    Agent::Gemini,
    Agent::Antigravity,
    Agent::Cursor,
    Agent::Copilot,
    Agent::OpenCode,
    Agent::Amp,
    Agent::Hermes,
];

impl Agent {
    pub fn from_name(name: &str) -> Option<Agent> {
        ALL.into_iter().find(|agent| agent.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Gemini => "gemini",
            Agent::Antigravity => "antigravity",
            Agent::Cursor => "cursor",
            Agent::Copilot => "copilot",
            Agent::OpenCode => "opencode",
            Agent::Amp => "amp",
            Agent::Hermes => "hermes",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex",
            Agent::Gemini => "Gemini CLI",
            Agent::Antigravity => "Antigravity",
            Agent::Cursor => "Cursor",
            Agent::Copilot => "Copilot CLI",
            Agent::OpenCode => "OpenCode",
            Agent::Amp => "Amp",
            Agent::Hermes => "Hermes",
        }
    }

    /// The command that starts it in a terminal.
    pub fn program(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Gemini => "gemini",
            Agent::Antigravity => "agy",
            Agent::Cursor => "cursor-agent",
            Agent::Copilot => "copilot",
            Agent::OpenCode => "opencode",
            Agent::Amp => "amp",
            Agent::Hermes => "hermes",
        }
    }

    /// Where its hooks are configured (a plugin file for OpenCode and Amp).
    pub fn config_path(self) -> PathBuf {
        let home = home();
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        match self {
            Agent::Claude => home.join(".claude/settings.json"),
            Agent::Codex => home.join(".codex/hooks.json"),
            Agent::Gemini => home.join(".gemini/settings.json"),
            Agent::Antigravity => home.join(".gemini/antigravity-cli/settings.json"),
            Agent::Cursor => home.join(".cursor/hooks.json"),
            Agent::Copilot => home.join(".copilot/hooks/arnotch.json"),
            Agent::OpenCode => config.join("opencode/plugins/arnotch.js"),
            Agent::Amp => config.join("amp/plugins/arnotch.ts"),
            Agent::Hermes => home.join(".hermes/config.yaml"),
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// An agent as the settings show it.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub name: &'static str,
    pub label: &'static str,
    /// Its program is on the PATH.
    pub installed: bool,
    /// Its configuration calls arnotchctl.
    pub connected: bool,
}

pub fn statuses() -> Vec<Status> {
    ALL.into_iter()
        .map(|agent| Status {
            name: agent.name(),
            label: agent.label(),
            installed: crate::apps::on_path(agent.program()),
            connected: is_connected(agent),
        })
        .collect()
}

pub fn is_connected(agent: Agent) -> bool {
    std::fs::read_to_string(agent.config_path()).is_ok_and(|text| text.contains("arnotchctl"))
}

/// Opens a terminal in `dir` running `agent`, through the user's login shell so its PATH
/// (npm, bun, ~/.local/bin) applies; the shell stays open when the agent quits.
pub fn launch(agent: Agent, dir: &Path) -> anyhow::Result<()> {
    use std::process::{Command, Stdio};
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let script = format!("{}; exec {shell}", agent.program());
    let shell_args = [shell.as_str(), "-lic", script.as_str()];
    let dir_text = dir.display().to_string();
    let terminals: [(&str, Vec<&str>); 6] = [
        ("konsole", vec!["--workdir", &dir_text, "-e"]),
        ("kitty", vec!["--directory", &dir_text]),
        ("wezterm", vec!["start", "--cwd", &dir_text, "--"]),
        ("alacritty", vec!["--working-directory", &dir_text, "-e"]),
        ("foot", vec!["--working-directory", &dir_text]),
        (
            "gnome-terminal",
            vec!["--working-directory", &dir_text, "--"],
        ),
    ];
    for (terminal, args) in terminals {
        if !crate::apps::on_path(terminal) {
            continue;
        }
        Command::new(terminal)
            .args(args)
            .args(shell_args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }
    anyhow::bail!("no terminal found (Konsole, kitty, WezTerm, Alacritty, foot, GNOME Terminal)")
}

/// The hook payload of `agent` as an island event, `None` for what the island doesn't
/// show. The event's `agent` names who sent it.
pub fn event_from_hook(agent: Agent, json: &str) -> Option<Event> {
    let input: Value = serde_json::from_str(json).ok()?;
    let claude_shaped = normalize(agent, &input)?;
    let mut event = claude::event_from_hook(&claude_shaped.to_string()).ok()??;
    if let Event::Agent { agent: who, .. } = &mut event {
        *who = if agent == Agent::Claude {
            String::new()
        } else {
            agent.name().to_owned()
        };
    }
    Some(event)
}

/// What the hook prints so `agent` applies the user's reply; `None` lets the agent ask
/// in its terminal as usual.
pub fn permission_output(agent: Agent, reply: &Reply, json: &str) -> Option<String> {
    let reason = || {
        reply
            .message
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| crate::i18n::tr("Denied from arnotch").into())
    };
    let output = match agent {
        Agent::Claude | Agent::Codex | Agent::OpenCode | Agent::Amp => {
            let input: Value = serde_json::from_str(json).ok()?;
            let claude_shaped = normalize(agent, &input)?;
            return claude::permission_output(reply, &claude_shaped.to_string());
        }
        Agent::Gemini | Agent::Antigravity => match reply.decision {
            Decision::Allow => json!({ "decision": "allow" }),
            Decision::Deny => json!({ "decision": "deny", "reason": reason() }),
            Decision::Ask => return None,
        },
        Agent::Cursor => match reply.decision {
            Decision::Allow => json!({ "permission": "allow" }),
            Decision::Deny => {
                json!({ "permission": "deny", "user_message": reason(), "agent_message": reason() })
            }
            // Cursor takes no output as a failed hook; asking is what it does anyway.
            Decision::Ask => json!({ "permission": "ask" }),
        },
        Agent::Copilot => match reply.decision {
            Decision::Allow => json!({ "behavior": "allow" }),
            Decision::Deny => json!({ "behavior": "deny", "message": reason() }),
            Decision::Ask => return None,
        },
        // Hermes' shell hooks can only block; anything else lets the call through.
        Agent::Hermes => match reply.decision {
            Decision::Allow => json!({}),
            Decision::Deny => json!({ "decision": "block", "reason": reason() }),
            Decision::Ask => return None,
        },
    };
    Some(output.to_string())
}

/// What the hook prints for an event that needs no answer: Cursor takes empty output from
/// some hooks as a failure.
pub fn passive_output(agent: Agent, json: &str) -> Option<&'static str> {
    if agent != Agent::Cursor {
        return None;
    }
    let event = serde_json::from_str::<Value>(json).ok()?;
    match event.get("hook_event_name").and_then(Value::as_str) {
        Some("beforeSubmitPrompt") => Some(r#"{"continue":true}"#),
        _ => Some("{}"),
    }
}

/// Claude Code's hook payload for what `input` from `agent` says, `None` when Claude Code
/// has no such event.
fn normalize(agent: Agent, input: &Value) -> Option<Value> {
    let text = |keys: &[&str]| -> String {
        keys.iter()
            .find_map(|key| input.get(*key).and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned()
    };
    let event = text(&["hook_event_name", "hookEventName", "event"]);
    let session = text(&[
        "session_id",
        "sessionId",
        "conversation_id",
        "conversationId",
    ]);
    let mut cwd = text(&["cwd"]);
    if cwd.is_empty() {
        cwd = input
            .get("workspace_roots")
            .and_then(|roots| roots.get(0))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
    }
    let tool = text(&["tool_name", "toolName"]);
    let tool_input = arguments(
        input
            .get("tool_input")
            .or_else(|| input.get("toolArgs"))
            .or_else(|| input.get("toolInput"))
            .unwrap_or(&Value::Null),
    );
    let tool_use_id = text(&["tool_use_id", "toolUseId", "tool_call_id"]);

    let mut out = json!({
        "session_id": session,
        "cwd": cwd,
        "tool_use_id": tool_use_id,
    });
    let set_tool = |out: &mut Value, tool: &str, tool_input: &Value| {
        let (name, input) = claude_tool(tool, tool_input);
        out["tool_name"] = name.into();
        out["tool_input"] = input;
    };

    let claude_event = match agent {
        // Codex, OpenCode and Amp (through arnotch's plugins) speak Claude Code's format.
        Agent::Claude | Agent::Codex | Agent::OpenCode | Agent::Amp => {
            let mut same = input.clone();
            if !tool.is_empty() {
                let (name, renamed) = claude_tool(&tool, &tool_input);
                same["tool_name"] = name.into();
                same["tool_input"] = renamed;
            }
            if same.get("session_id").is_none() {
                same["session_id"] = session.into();
            }
            return Some(same);
        }
        Agent::Gemini | Agent::Antigravity => match event.as_str() {
            "BeforeAgent" | "UserPromptSubmit" => {
                out["prompt"] = text(&["prompt"]).into();
                "UserPromptSubmit"
            }
            // Gemini asks before commands and file changes; the island asks instead.
            "BeforeTool" | "PreToolUse" => {
                set_tool(&mut out, &tool, &tool_input);
                if changes_things(out["tool_name"].as_str().unwrap_or_default()) {
                    "PermissionRequest"
                } else {
                    "PreToolUse"
                }
            }
            "AfterTool" | "PostToolUse" => {
                set_tool(&mut out, &tool, &tool_input);
                out["tool_response"] = input.get("tool_response").cloned().unwrap_or_default();
                "PostToolUse"
            }
            "AfterAgent" | "Stop" => "Stop",
            "SessionEnd" => "SessionEnd",
            "Notification" if text(&["notification_type"]) == "ToolPermission" => {
                out["message"] = text(&["message"]).into();
                out["notification_type"] = "permission_prompt".into();
                "Notification"
            }
            _ => return None,
        },
        Agent::Cursor => match event.as_str() {
            "beforeSubmitPrompt" => {
                out["prompt"] = text(&["prompt"]).into();
                "UserPromptSubmit"
            }
            "beforeShellExecution" => {
                set_tool(&mut out, "Bash", &json!({ "command": text(&["command"]) }));
                "PermissionRequest"
            }
            "beforeMCPExecution" => {
                set_tool(&mut out, &tool, &tool_input);
                "PermissionRequest"
            }
            "preToolUse" => {
                set_tool(&mut out, &tool, &tool_input);
                "PreToolUse"
            }
            "postToolUse" | "postToolUseFailure" | "afterShellExecution" => {
                set_tool(&mut out, &tool, &tool_input);
                if event == "postToolUseFailure" {
                    out["error"] = text(&["error"]).into();
                    "PostToolUseFailure"
                } else {
                    out["tool_response"] = input
                        .get("tool_output")
                        .or_else(|| input.get("output"))
                        .cloned()
                        .unwrap_or_default();
                    "PostToolUse"
                }
            }
            "afterFileEdit" => {
                set_tool(
                    &mut out,
                    "Edit",
                    &json!({ "file_path": text(&["file_path", "path"]) }),
                );
                "PostToolUse"
            }
            "stop" => "Stop",
            "sessionEnd" => "SessionEnd",
            _ => return None,
        },
        Agent::Copilot => match event.as_str() {
            "" | "userPromptSubmitted" | "UserPromptSubmit" if input.get("prompt").is_some() => {
                out["prompt"] = text(&["prompt"]).into();
                "UserPromptSubmit"
            }
            "preToolUse" | "PreToolUse" => {
                set_tool(&mut out, &tool, &tool_input);
                "PreToolUse"
            }
            "postToolUse" | "PostToolUse" => {
                set_tool(&mut out, &tool, &tool_input);
                out["tool_response"] = input
                    .get("toolResult")
                    .or_else(|| input.get("tool_response"))
                    .cloned()
                    .unwrap_or_default();
                "PostToolUse"
            }
            "postToolUseFailure" | "PostToolUseFailure" => {
                set_tool(&mut out, &tool, &tool_input);
                out["error"] = text(&["error"]).into();
                "PostToolUseFailure"
            }
            "permissionRequest" | "PermissionRequest" => {
                set_tool(&mut out, &tool, &tool_input);
                "PermissionRequest"
            }
            "agentStop" | "Stop" => "Stop",
            "sessionEnd" | "SessionEnd" => "SessionEnd",
            "notification" | "Notification" => {
                if text(&["notification_type"]) != "permission_prompt" {
                    return None;
                }
                out["message"] = text(&["message"]).into();
                out["notification_type"] = "permission_prompt".into();
                "Notification"
            }
            _ => return None,
        },
        Agent::Hermes => {
            let extra = input.get("extra").cloned().unwrap_or_default();
            let extra_text = |key: &str| extra.get(key).and_then(Value::as_str).unwrap_or_default();
            if out["tool_use_id"].as_str() == Some("") {
                out["tool_use_id"] = extra_text("tool_call_id").into();
            }
            match event.as_str() {
                "pre_llm_call" => {
                    out["prompt"] = extra_text("user_message").into();
                    "UserPromptSubmit"
                }
                // Hermes runs commands without asking; the island asks before the risky ones.
                "pre_tool_call" => {
                    set_tool(&mut out, &tool, &tool_input);
                    let command = out["tool_input"]["command"].as_str().unwrap_or_default();
                    if out["tool_name"] == "Bash" && risky_command(command) {
                        "PermissionRequest"
                    } else {
                        "PreToolUse"
                    }
                }
                "post_tool_call" => {
                    set_tool(&mut out, &tool, &tool_input);
                    out["tool_response"] = hermes_result(extra.get("result"));
                    "PostToolUse"
                }
                "post_llm_call" => "Stop",
                "on_session_finalize" => "SessionEnd",
                _ => return None,
            }
        }
    };
    out["hook_event_name"] = claude_event.into();
    Some(out)
}

/// Tool arguments, which some agents send as a JSON string.
fn arguments(value: &Value) -> Value {
    match value {
        Value::String(text) => {
            serde_json::from_str(text).unwrap_or_else(|_| json!({ "command": text }))
        }
        other => other.clone(),
    }
}

/// Claude Code's name and input fields for another agent's tool, so the island describes
/// it the same way (the command, the file, a diff of an edit).
fn claude_tool(tool: &str, input: &Value) -> (String, Value) {
    let field = |keys: &[&str]| -> Value {
        keys.iter()
            .find_map(|key| input.get(*key).filter(|v| !v.is_null()).cloned())
            .unwrap_or_default()
    };
    let path = || {
        field(&[
            "file_path",
            "path",
            "absolute_path",
            "filePath",
            "target_file",
        ])
    };
    match tool.to_lowercase().as_str() {
        "bash" | "shell" | "exec_command" | "local_shell" | "run_shell_command"
        | "run_terminal_cmd" | "terminal" | "shell_command" | "execute" | "powershell" => {
            let command = match field(&["command", "cmd"]) {
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
                Value::String(text) => text,
                _ => String::new(),
            };
            // `bash -lc "..."` as Codex sends it: the script is what matters.
            let command = command
                .strip_prefix("bash -lc ")
                .map(|rest| rest.trim_matches(['"', '\'']).to_owned())
                .unwrap_or(command);
            ("Bash".into(), json!({ "command": command }))
        }
        "edit" | "replace" | "edit_file" | "str_replace" | "str_replace_editor" | "patch"
        | "search_replace" | "multi_edit" => (
            "Edit".into(),
            json!({
                "file_path": path(),
                "old_string": field(&["old_string", "old_str", "oldString", "old_text"]),
                "new_string": field(&["new_string", "new_str", "newString", "new_text"]),
            }),
        ),
        "write" | "write_file" | "create" | "create_file" => (
            "Write".into(),
            json!({ "file_path": path(), "content": field(&["content", "file_text", "contents"]) }),
        ),
        "read" | "read_file" | "view" | "read_many_files" => {
            ("Read".into(), json!({ "file_path": path() }))
        }
        "grep" | "search_files" | "search_file_content" | "glob" => (
            "Grep".into(),
            json!({ "pattern": field(&["pattern", "query", "regex"]) }),
        ),
        "web_search" | "google_web_search" | "websearch" => {
            ("WebSearch".into(), json!({ "query": field(&["query"]) }))
        }
        "web_fetch" | "webfetch" | "fetch" | "web_extract" => (
            "WebFetch".into(),
            json!({ "url": field(&["url", "urls", "prompt"]) }),
        ),
        "apply_patch" => {
            // "*** Update File: src/main.rs" lines name the files the patch touches.
            let patch = field(&["input", "patch"]);
            let file = patch
                .as_str()
                .unwrap_or_default()
                .lines()
                .find_map(|line| {
                    ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                        .iter()
                        .find_map(|prefix| line.strip_prefix(prefix))
                })
                .unwrap_or_default()
                .to_owned();
            ("Edit".into(), json!({ "file_path": file }))
        }
        _ => (tool.to_owned(), input.clone()),
    }
}

/// Tools that run commands or change files, which the island asks about for agents that
/// would ask in their terminal.
fn changes_things(tool: &str) -> bool {
    matches!(tool, "Bash" | "Edit" | "Write")
}

/// Commands worth a look before Hermes runs them: they delete, overwrite, publish or need
/// root. Everything else runs without asking, as Hermes does on its own.
fn risky_command(command: &str) -> bool {
    let command = command.to_lowercase();
    let words: Vec<&str> = command
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')'))
        .filter(|word| !word.is_empty())
        .collect();
    let has = |word: &str| words.contains(&word);
    has("rm")
        || has("sudo")
        || has("doas")
        || has("dd")
        || has("mkfs")
        || has("shred")
        || has("chmod")
        || has("chown")
        || has("kill")
        || has("pkill")
        || has("killall")
        || has("reboot")
        || has("shutdown")
        || command.contains("git push")
        || command.contains("git reset --hard")
        || command.contains("git clean")
        || command.contains("> /dev/")
        || ((command.contains("curl") || command.contains("wget"))
            && (command.contains("| sh") || command.contains("| bash")))
}

/// Hermes' tool result: a JSON string with the output, or plain text.
fn hermes_result(result: Option<&Value>) -> Value {
    let Some(Value::String(text)) = result else {
        return result.cloned().unwrap_or_default();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(fields)) => {
            let output = ["output", "stdout", "content", "result"]
                .iter()
                .find_map(|key| fields.get(*key).and_then(Value::as_str))
                .unwrap_or_default()
                .to_owned();
            let failed = fields
                .get("exit_code")
                .and_then(Value::as_i64)
                .is_some_and(|code| code != 0)
                || fields.get("error").is_some_and(|e| !e.is_null() && e != "");
            json!({ "stdout": output, "is_error": failed })
        }
        _ => json!({ "stdout": text }),
    }
}

/// Adds arnotch's hooks to `agent`'s configuration, keeping everything else. `arnotchctl`
/// is the absolute path of the program the hooks call. Returns where they were written.
pub fn connect(agent: Agent, arnotchctl: &Path) -> anyhow::Result<PathBuf> {
    let path = agent.config_path();
    let command = format!("'{}' agent-hook {}", arnotchctl.display(), agent.name());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match agent {
        Agent::Claude => anyhow::bail!("Claude Code is connected by contrib/install.sh --hooks"),
        Agent::Codex => update_json(&path, |config| {
            for (event, tool_event, timeout) in [
                ("UserPromptSubmit", false, 10),
                ("PreToolUse", true, 10),
                ("PostToolUse", true, 10),
                ("PermissionRequest", true, 600),
                ("Stop", false, 10),
                ("SessionEnd", false, 2),
            ] {
                let mut group = json!({ "hooks": [{ "type": "command", "command": command, "timeout": timeout }] });
                if tool_event {
                    group["matcher"] = "*".into();
                }
                add_group(config, event, group);
            }
        }),
        Agent::Gemini | Agent::Antigravity => update_json(&path, |config| {
            for (event, tool_event, timeout_ms) in [
                ("BeforeAgent", false, 10_000),
                ("BeforeTool", true, 600_000),
                ("AfterTool", true, 10_000),
                ("AfterAgent", false, 10_000),
                ("SessionEnd", false, 5_000),
                ("Notification", false, 10_000),
            ] {
                let mut group = json!({ "hooks": [{ "type": "command", "name": "arnotch", "command": command, "timeout": timeout_ms }] });
                if tool_event {
                    group["matcher"] = "*".into();
                }
                add_group(config, event, group);
            }
        }),
        Agent::Cursor => update_json(&path, |config| {
            config["version"] = 1.into();
            for (event, timeout) in [
                ("beforeSubmitPrompt", 10),
                ("beforeShellExecution", 600),
                ("beforeMCPExecution", 600),
                ("preToolUse", 10),
                ("postToolUse", 10),
                ("postToolUseFailure", 10),
                ("afterFileEdit", 10),
                ("stop", 10),
                ("sessionEnd", 5),
            ] {
                add_group(
                    config,
                    event,
                    json!({ "command": command, "timeout": timeout }),
                );
            }
        }),
        Agent::Copilot => update_json(&path, |config| {
            config["version"] = 1.into();
            for (event, timeout) in [
                ("userPromptSubmitted", 10),
                ("preToolUse", 10),
                ("postToolUse", 10),
                ("postToolUseFailure", 10),
                ("permissionRequest", 600),
                ("agentStop", 10),
                ("sessionEnd", 5),
                ("notification", 10),
            ] {
                add_group(
                    config,
                    event,
                    json!({ "type": "command", "bash": command, "timeoutSec": timeout }),
                );
            }
        }),
        Agent::OpenCode => std::fs::write(&path, plugin(OPENCODE_PLUGIN, arnotchctl)),
        Agent::Amp => std::fs::write(&path, plugin(AMP_PLUGIN, arnotchctl)),
        Agent::Hermes => connect_hermes(&path, arnotchctl),
    }?;
    Ok(path)
}

/// Reads a JSON config (an empty object when there is none), lets `change` edit it and
/// writes it back, after keeping a copy of the old file next to it.
fn update_json(path: &Path, change: impl FnOnce(&mut Value)) -> std::io::Result<()> {
    let mut config = match std::fs::read_to_string(path) {
        Ok(text) => {
            std::fs::write(path.with_extension("json.arnotch-backup"), &text)?;
            serde_json::from_str(&text).map_err(std::io::Error::other)?
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(err) => return Err(err),
    };
    if !config.is_object() {
        return Err(std::io::Error::other(format!(
            "{} is not a JSON object",
            path.display()
        )));
    }
    change(&mut config);
    let text = serde_json::to_string_pretty(&config).map_err(std::io::Error::other)?;
    std::fs::write(path, text + "\n")
}

/// Adds `group` to `config.hooks[event]`, replacing an earlier arnotch entry.
fn add_group(config: &mut Value, event: &str, group: Value) {
    if !config["hooks"].is_object() {
        config["hooks"] = json!({});
    }
    let list = &mut config["hooks"][event];
    if !list.is_array() {
        *list = json!([]);
    }
    let list = list.as_array_mut().expect("just made an array");
    list.retain(|existing| !existing.to_string().contains("arnotchctl"));
    list.push(group);
}

/// Hermes' config is YAML: the hooks are added as a block of their own, which only works
/// while there is no `hooks:` section yet (there is no YAML library here to merge one).
fn connect_hermes(path: &Path, arnotchctl: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if text.contains("arnotchctl") {
        return Ok(());
    }
    if text
        .lines()
        .any(|line| line.trim_end() == "hooks:" || line.starts_with("hooks: "))
    {
        return Err(std::io::Error::other(format!(
            "{} already has a hooks: section; add arnotch's by hand (see the README)",
            path.display()
        )));
    }
    std::fs::write(path.with_extension("yaml.arnotch-backup"), &text)?;
    let command = format!("{} agent-hook hermes", arnotchctl.display());
    let mut block =
        String::from("\n# Added by arnotch: shows Hermes in the notch and asks there.\nhooks:\n");
    for (event, timeout) in [
        ("pre_llm_call", 10),
        ("pre_tool_call", 300),
        ("post_tool_call", 10),
        ("post_llm_call", 10),
        ("on_session_finalize", 5),
    ] {
        block.push_str(&format!(
            "  {event}:\n    - command: \"{command}\"\n      timeout: {timeout}\n"
        ));
    }
    let mut text = text;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    std::fs::write(path, text + &block)
}

fn plugin(template: &str, arnotchctl: &Path) -> String {
    template.replace("@ARNOTCHCTL@", &arnotchctl.display().to_string())
}

/// OpenCode plugin: sends Claude Code shaped hook payloads to arnotchctl and applies the
/// island's answer to permission requests.
const OPENCODE_PLUGIN: &str = include_str!("../../../contrib/agents/opencode.js");
/// Amp plugin, likewise.
const AMP_PLUGIN: &str = include_str!("../../../contrib/agents/amp.ts");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::AgentPhase;

    fn phase(agent: Agent, json: &str) -> (String, AgentPhase) {
        match event_from_hook(agent, json) {
            Some(Event::Agent { agent, phase, .. }) => (agent, phase),
            other => panic!("not an agent event: {other:?}"),
        }
    }

    #[test]
    fn codex_permission_requests_ask_in_the_island() {
        let json = r#"{"hook_event_name":"PermissionRequest","session_id":"c1","cwd":"/home/u/korus",
            "tool_name":"Bash","tool_input":{"command":["bash","-lc","cargo test"]},"tool_use_id":"t1"}"#;
        let (agent, phase) = phase(Agent::Codex, json);
        assert_eq!(agent, "codex");
        assert!(
            matches!(&phase, AgentPhase::Permission { tool, detail, .. } if tool == "Bash" && detail == "cargo test")
        );
        let allow = permission_output(Agent::Codex, &Decision::Allow.into(), json).unwrap();
        assert!(allow.contains(r#""behavior":"allow""#), "{allow}");
        assert!(allow.contains("PermissionRequest"));
    }

    #[test]
    fn gemini_asks_before_commands_but_not_reads() {
        let shell = r#"{"hook_event_name":"BeforeTool","session_id":"g1","cwd":"/p",
            "tool_name":"run_shell_command","tool_input":{"command":"npm i"}}"#;
        assert!(matches!(
            phase(Agent::Gemini, shell).1,
            AgentPhase::Permission { .. }
        ));
        let read = r#"{"hook_event_name":"BeforeTool","session_id":"g1","cwd":"/p",
            "tool_name":"read_file","tool_input":{"absolute_path":"/p/src/lib.rs"}}"#;
        assert!(
            matches!(phase(Agent::Gemini, read).1, AgentPhase::Tool { detail, .. } if detail == "lib.rs")
        );
        assert_eq!(
            permission_output(Agent::Gemini, &Decision::Allow.into(), shell).as_deref(),
            Some(r#"{"decision":"allow"}"#)
        );
        assert_eq!(
            permission_output(Agent::Gemini, &Decision::Ask.into(), shell),
            None
        );
        let prompt =
            r#"{"hook_event_name":"BeforeAgent","session_id":"g1","cwd":"/p","prompt":"fix it"}"#;
        assert!(
            matches!(phase(Agent::Gemini, prompt).1, AgentPhase::Started { prompt } if prompt == "fix it")
        );
    }

    #[test]
    fn cursor_shell_commands_and_answers() {
        let json = r#"{"hook_event_name":"beforeShellExecution","conversation_id":"k1",
            "command":"git push","cwd":"","workspace_roots":["/home/u/site"]}"#;
        match event_from_hook(Agent::Cursor, json) {
            Some(Event::Agent {
                session_id,
                project,
                phase: AgentPhase::Permission { detail, .. },
                ..
            }) => {
                assert_eq!(session_id, "k1");
                assert_eq!(project, "site");
                assert_eq!(detail, "git push");
            }
            other => panic!("{other:?}"),
        }
        let deny = Reply {
            decision: Decision::Deny,
            message: Some("not now".into()),
            ..Default::default()
        };
        let out: Value =
            serde_json::from_str(&permission_output(Agent::Cursor, &deny, json).unwrap()).unwrap();
        assert_eq!(out["permission"], "deny");
        assert_eq!(out["agent_message"], "not now");
        assert_eq!(
            passive_output(Agent::Cursor, r#"{"hook_event_name":"beforeSubmitPrompt"}"#),
            Some(r#"{"continue":true}"#)
        );
    }

    #[test]
    fn copilot_camel_case_payloads() {
        let json = r#"{"hookEventName":"permissionRequest","sessionId":"p1","cwd":"/w/app",
            "toolName":"bash","toolArgs":"{\"command\":\"make\"}"}"#;
        assert!(
            matches!(phase(Agent::Copilot, json).1, AgentPhase::Permission { detail, .. } if detail == "make")
        );
        assert_eq!(
            permission_output(Agent::Copilot, &Decision::Allow.into(), json).as_deref(),
            Some(r#"{"behavior":"allow"}"#)
        );
    }

    #[test]
    fn hermes_asks_only_before_risky_commands() {
        let risky = r#"{"hook_event_name":"pre_tool_call","tool_name":"terminal","tool_input":{"command":"rm -rf build"},
            "session_id":"h1","cwd":"/home/u/bot","extra":{"tool_call_id":"x1"}}"#;
        assert!(matches!(
            phase(Agent::Hermes, risky).1,
            AgentPhase::Permission { .. }
        ));
        let safe = r#"{"hook_event_name":"pre_tool_call","tool_name":"terminal","tool_input":{"command":"ls -la"},
            "session_id":"h1","cwd":"/home/u/bot","extra":{}}"#;
        assert!(matches!(
            phase(Agent::Hermes, safe).1,
            AgentPhase::Tool { .. }
        ));
        assert_eq!(
            permission_output(Agent::Hermes, &Decision::Allow.into(), risky).as_deref(),
            Some("{}")
        );
        let deny = permission_output(Agent::Hermes, &Decision::Deny.into(), risky).unwrap();
        assert!(deny.contains(r#""decision":"block""#));

        let result = r#"{"hook_event_name":"post_tool_call","tool_name":"terminal","tool_input":{"command":"false"},
            "session_id":"h1","cwd":"/b","extra":{"result":"{\"output\":\"boom\",\"exit_code\":1}"}}"#;
        assert!(matches!(
            phase(Agent::Hermes, result).1,
            AgentPhase::ToolResult { ok: false, .. }
        ));
        let prompt = r#"{"hook_event_name":"pre_llm_call","session_id":"h1","cwd":"/b","extra":{"user_message":"merhaba"}}"#;
        assert!(
            matches!(phase(Agent::Hermes, prompt).1, AgentPhase::Started { prompt } if prompt == "merhaba")
        );
        let done = r#"{"hook_event_name":"post_llm_call","session_id":"h1","cwd":"/b","extra":{}}"#;
        assert!(matches!(phase(Agent::Hermes, done).1, AgentPhase::Done));
    }

    #[test]
    fn risky_commands() {
        assert!(risky_command("sudo pacman -Syu"));
        assert!(risky_command("cd x && rm file"));
        assert!(risky_command("curl -fsSL x.sh | sh"));
        assert!(!risky_command("ls; cat README.md"));
        assert!(!risky_command("cargo fmt"));
    }

    #[test]
    fn claude_events_keep_an_empty_agent() {
        let json = r#"{"hook_event_name":"Stop","session_id":"s","cwd":"/p"}"#;
        assert_eq!(phase(Agent::Claude, json).0, "");
    }

    #[test]
    fn hooks_are_added_once_beside_others() {
        let mut config = json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "notify-send hi" }] }] } });
        add_group(
            &mut config,
            "Stop",
            json!({ "hooks": [{ "command": "'/x/arnotchctl' agent-hook codex" }] }),
        );
        add_group(
            &mut config,
            "Stop",
            json!({ "hooks": [{ "command": "'/x/arnotchctl' agent-hook codex" }] }),
        );
        assert_eq!(config["hooks"]["Stop"].as_array().unwrap().len(), 2);
    }
}
