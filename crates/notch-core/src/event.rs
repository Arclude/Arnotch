use serde::{Deserialize, Serialize};

/// Everything the notch can display. Sent over the socket as one JSON object per line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A generic message, e.g. from a script: `notchctl notify "Build" "done"`.
    Notify {
        title: String,
        #[serde(default)]
        body: String,
    },
    /// Progress of a coding agent session (Claude Code, ...).
    Agent {
        session_id: String,
        /// Short project name, usually the basename of the working directory.
        project: String,
        #[serde(flatten)]
        phase: AgentPhase,
    },
    /// Ask the notch to show its settings window.
    OpenSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum AgentPhase {
    /// The user submitted a prompt; the agent is thinking.
    Started {
        #[serde(default)]
        prompt: String,
    },
    /// The agent is about to run a tool.
    Tool {
        tool: String,
        detail: String,
        /// What a file edit is going to change.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<Diff>,
    },
    /// A tool call finished.
    ToolResult {
        tool: String,
        ok: bool,
        /// The last lines of the tool's output (commands only).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        output: Vec<String>,
    },
    /// The agent needs the user (permission prompt, idle input, ...).
    Waiting { message: String },
    /// The agent finished its turn.
    Done,
    /// The session was closed.
    Ended,
}

/// A short excerpt of a file change, ready to display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Diff {
    pub file: String,
    pub lines: Vec<DiffLine>,
    /// More changed lines exist than `lines` shows.
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// 1-based line number in the file (the old file for removals), 0 when unknown.
    pub n: u32,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Add,
    Del,
    Ctx,
}

impl Event {
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).expect("event is always serializable");
        line.push('\n');
        line
    }
}
