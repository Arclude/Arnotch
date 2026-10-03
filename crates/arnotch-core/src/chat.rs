//! Chat with Claude from the island. Each message runs the Claude Code CLI in print
//! mode (`claude -p`), so it uses the user's own login and needs no API key; the
//! conversation continues with `--resume`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, bail};
use serde::Deserialize;

/// Keeps answers short enough for the island.
const SYSTEM_PROMPT: &str = "You are answering in a small chat panel at the top of the user's \
Linux desktop. Keep answers brief and to the point; use Markdown sparingly. You have no tools.";

/// A running answer. Read it with [`Answer::stream`]; [`Answer::pid`] lets another thread stop it.
pub struct Answer {
    child: Child,
}

/// Starts answering `prompt` in the conversation `session_id`: a new one, or one that
/// already has messages when `resume` is set.
pub fn ask(session_id: &str, resume: bool, prompt: &str) -> anyhow::Result<Answer> {
    let dir = chat_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

    let mut command = Command::new(claude_binary());
    command
        .current_dir(&dir)
        .args([
            "-p",
            "--output-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
        ])
        // A plain chat: no tools, no MCP servers, and none of the user's settings, whose
        // hooks would show this chat in the island as one more agent session.
        .args([
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "",
        ])
        .args(["--append-system-prompt", SYSTEM_PROMPT])
        .args([if resume { "--resume" } else { "--session-id" }, session_id])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("cannot start claude (is Claude Code installed?)")?;

    // The prompt goes through stdin, so it can be long or start with a dash.
    let mut stdin = child.stdin.take().expect("stdin is piped");
    stdin.write_all(prompt.as_bytes())?;
    drop(stdin);
    Ok(Answer { child })
}

impl Answer {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Calls `on_text` with each piece of the answer as it arrives, until it is complete.
    pub fn stream(mut self, mut on_text: impl FnMut(&str)) -> anyhow::Result<()> {
        let stdout = BufReader::new(self.child.stdout.take().expect("stdout is piped"));
        let mut failure = None;
        let mut finished = false;
        for line in stdout.lines() {
            let line = line?;
            match serde_json::from_str::<Message>(&line) {
                Ok(Message::StreamEvent {
                    event:
                        StreamEvent::ContentBlockDelta {
                            delta: Delta::TextDelta { text },
                        },
                }) => on_text(&text),
                Ok(Message::Result { is_error, result }) => {
                    finished = true;
                    if is_error {
                        failure = Some(result.unwrap_or_else(|| "Claude returned an error".into()));
                    }
                }
                _ => {}
            }
        }

        let status = self.child.wait()?;
        if let Some(message) = failure {
            bail!(message);
        }
        if !finished {
            let mut stderr = String::new();
            if let Some(mut err) = self.child.stderr.take() {
                _ = std::io::Read::read_to_string(&mut err, &mut stderr);
            }
            let stderr = stderr.trim();
            bail!(if stderr.is_empty() {
                format!("claude exited ({status})")
            } else {
                stderr.to_owned()
            });
        }
        Ok(())
    }
}

/// A new conversation id.
pub fn new_session_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/uuid")
        .map(|id| id.trim().to_owned())
        .unwrap_or_else(|_| {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let hex = format!("{nanos:032x}");
            format!(
                "{}-{}-4{}-8{}-{}",
                &hex[0..8],
                &hex[8..12],
                &hex[13..16],
                &hex[17..20],
                &hex[20..32]
            )
        })
}

/// Whether Claude Code saved the conversation `session_id`, so it can be resumed.
/// (A first message that failed early leaves nothing to resume.)
pub fn saved(session_id: &str) -> bool {
    let config = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"));
    let file = format!("{session_id}.jsonl");
    std::fs::read_dir(config.join("projects"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|project| project.path().join(&file).is_file())
}

/// Stops the answer being written by the `claude` process `pid`.
pub fn stop(pid: u32) {
    // SAFETY: kill has no memory-safety preconditions.
    unsafe { libc::kill(pid as i32, libc::SIGTERM) };
}

/// Where the chat runs: an empty folder, so no project's CLAUDE.md is picked up.
fn chat_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"));
    data.join("arnotch/chat")
}

/// `claude` from PATH, or where its installer puts it: a systemd service may not have
/// ~/.local/bin on its PATH.
fn claude_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("ARNOTCH_CLAUDE") {
        return path.into();
    }
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join("claude"));
    let installed = [".local/bin/claude", ".claude/local/claude"].map(|p| home().join(p));
    on_path
        .chain(installed)
        .find(|path| path.is_file())
        .unwrap_or_else(|| "claude".into())
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The parts of `claude -p --output-format stream-json` the chat needs.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Message {
    StreamEvent {
        event: StreamEvent,
    },
    Result {
        #[serde(default)]
        is_error: bool,
        result: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamEvent {
    ContentBlockDelta {
        delta: Delta,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Delta {
    TextDelta {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_text_deltas_and_results() {
        let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Merhaba"}},"session_id":"s"}"#;
        assert!(matches!(
            serde_json::from_str::<Message>(delta).unwrap(),
            Message::StreamEvent { event: StreamEvent::ContentBlockDelta { delta: Delta::TextDelta { text } } } if text == "Merhaba"
        ));

        let thinking = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"..."}}}"#;
        assert!(matches!(
            serde_json::from_str::<Message>(thinking).unwrap(),
            Message::StreamEvent {
                event: StreamEvent::ContentBlockDelta {
                    delta: Delta::Other
                }
            }
        ));

        let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"Merhaba","session_id":"s"}"#;
        assert!(matches!(
            serde_json::from_str::<Message>(result).unwrap(),
            Message::Result {
                is_error: false,
                ..
            }
        ));

        let init = r#"{"type":"system","subtype":"init","tools":[]}"#;
        assert!(matches!(
            serde_json::from_str::<Message>(init).unwrap(),
            Message::Other
        ));
    }
}
