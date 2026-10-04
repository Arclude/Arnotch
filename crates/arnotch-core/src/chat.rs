//! Chat with Claude from the island. Each message runs the Claude Code CLI in print
//! mode (`claude -p`), so it uses the user's own login and needs no API key; the
//! conversation continues with `--resume`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, bail};
use serde::Deserialize;

/// Keeps answers short enough for the island.
const SYSTEM_PROMPT: &str = "You are answering in a small chat panel at the top of the user's \
Linux desktop. Keep answers brief and to the point; use Markdown sparingly. You can't change \
anything on the user's computer.";

/// Images up to this size go to Claude as they are; larger ones only by path.
const MAX_IMAGE: u64 = 5 * 1024 * 1024;
/// PDFs up to this size go to Claude as documents, which it reads page by page (the API
/// takes up to 32 MB and 100 pages); larger ones only by path.
const MAX_PDF: u64 = 20 * 1024 * 1024;
/// Text files up to this size go with their contents.
const MAX_TEXT: u64 = 200 * 1024;

/// A message to Claude.
#[derive(Debug, Default)]
pub struct Request {
    pub prompt: String,
    /// Files attached to the message.
    pub attachments: Vec<PathBuf>,
    /// Let Claude search and read the web (WebSearch, WebFetch).
    pub web: bool,
    /// Let Claude read files on this computer (Read, Glob, Grep), read-only.
    pub files: bool,
}

/// A piece of the answer as it streams in.
pub enum Piece<'a> {
    Text(&'a str),
    /// Claude started using a tool ("WebSearch", "Read", ...).
    Tool(&'a str),
}

/// A running answer. Read it with [`Answer::stream`]; [`Answer::pid`] lets another thread stop it.
pub struct Answer {
    child: Child,
}

/// Starts answering `prompt` in the conversation `session_id`: a new one, or one that
/// already has messages when `resume` is set.
pub fn ask(session_id: &str, resume: bool, request: &Request) -> anyhow::Result<Answer> {
    let dir = chat_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

    let mut command = Command::new(claude_binary());
    command
        .current_dir(&dir)
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
        ])
        // Only read-only tools the user turned on, allowed without asking; no MCP servers,
        // and none of the user's settings, whose hooks would show this chat in the island
        // as one more agent session.
        .args(["--strict-mcp-config", "--setting-sources", ""]);
    let mut tools = Vec::new();
    if request.web {
        tools.extend(["WebSearch", "WebFetch"]);
    }
    if request.files {
        tools.extend(["Read", "Glob", "Grep"]);
        command.args(["--add-dir", &home().to_string_lossy()]);
    }
    command
        .args(["--tools", &tools.join(",")])
        .args(["--allowedTools", &tools.join(",")])
        .args(["--append-system-prompt", SYSTEM_PROMPT])
        .args([if resume { "--resume" } else { "--session-id" }, session_id])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("cannot start claude (is Claude Code installed?)")?;

    // One user message, with the attachments as content blocks.
    let message = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": content(request) },
    });
    let mut stdin = child.stdin.take().expect("stdin is piped");
    writeln!(stdin, "{message}")?;
    drop(stdin);
    Ok(Answer { child })
}

/// The message's content blocks: the prompt, then each attachment as an image, a PDF
/// document or text, or by path when it is none of these (Claude can read it when file
/// reading is on).
fn content(request: &Request) -> Vec<serde_json::Value> {
    use base64::Engine;

    let mut blocks = vec![serde_json::json!({ "type": "text", "text": request.prompt })];
    for path in &request.attachments {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
        let name = path.display();
        if let Some(media_type) = image_type(path)
            && size <= MAX_IMAGE
            && let Ok(bytes) = std::fs::read(path)
        {
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            blocks.push(
                serde_json::json!({ "type": "text", "text": format!("Attached image: {name}") }),
            );
            blocks.push(serde_json::json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media_type, "data": data },
            }));
        } else if is_pdf(path)
            && size <= MAX_PDF
            && let Ok(bytes) = std::fs::read(path)
        {
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            blocks.push(
                serde_json::json!({ "type": "text", "text": format!("Attached PDF: {name}") }),
            );
            blocks.push(serde_json::json!({
                "type": "document",
                "source": { "type": "base64", "media_type": "application/pdf", "data": data },
            }));
        } else if let Some(text) = (size <= MAX_TEXT)
            .then(|| std::fs::read_to_string(path).ok())
            .flatten()
        {
            blocks.push(serde_json::json!({
                "type": "text",
                "text": format!("<file path=\"{name}\">\n{text}\n</file>"),
            }));
        } else {
            blocks.push(serde_json::json!({ "type": "text", "text": format!("Attached file (not included): {name}") }));
        }
    }
    blocks
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

fn image_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

impl Answer {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Calls `on_piece` with each piece of the answer as it arrives, until it is complete.
    pub fn stream(mut self, mut on_piece: impl FnMut(Piece)) -> anyhow::Result<()> {
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
                }) => on_piece(Piece::Text(&text)),
                Ok(Message::StreamEvent {
                    event:
                        StreamEvent::ContentBlockStart {
                            content_block: Block::ToolUse { name } | Block::ServerToolUse { name },
                        },
                }) => on_piece(Piece::Tool(&name)),
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
    ContentBlockStart {
        content_block: Block,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Block {
    ToolUse {
        name: String,
    },
    /// Tools the API runs itself, such as web search.
    ServerToolUse {
        name: String,
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
    fn attachments_become_content_blocks() {
        let dir = std::env::temp_dir().join(format!("arnotch-chat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (png, txt, bin, pdf) = (
            dir.join("shot.png"),
            dir.join("notes.txt"),
            dir.join("blob.bin"),
            dir.join("CV.PDF"),
        );
        std::fs::write(&png, [0x89, b'P', b'N', b'G']).unwrap();
        std::fs::write(&txt, "hello").unwrap();
        std::fs::write(&bin, [0xff, 0xfe, 0x00]).unwrap();
        std::fs::write(&pdf, b"%PDF").unwrap();

        let request = Request {
            prompt: "What is this?".into(),
            attachments: vec![png, txt, bin, pdf],
            ..Default::default()
        };
        let blocks = content(&request);
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(blocks[0]["text"], "What is this?");
        assert_eq!(blocks[2]["type"], "image");
        assert_eq!(blocks[2]["source"]["media_type"], "image/png");
        assert_eq!(blocks[2]["source"]["data"], "iVBORw==");
        assert!(
            blocks[3]["text"]
                .as_str()
                .unwrap()
                .ends_with("notes.txt\">\nhello\n</file>")
        );
        assert!(
            blocks[4]["text"]
                .as_str()
                .unwrap()
                .starts_with("Attached file (not included)")
        );
        assert_eq!(blocks[6]["type"], "document");
        assert_eq!(blocks[6]["source"]["media_type"], "application/pdf");
        assert_eq!(blocks[6]["source"]["data"], "JVBERg==");
    }

    #[test]
    fn reads_tool_use_starts() {
        let start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t","name":"WebSearch","input":{}}}}"#;
        assert!(matches!(
            serde_json::from_str::<Message>(start).unwrap(),
            Message::StreamEvent { event: StreamEvent::ContentBlockStart { content_block: Block::ToolUse { name } } } if name == "WebSearch"
        ));
    }

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
