use serde::{Deserialize, Serialize};

use crate::terminals::Terminal;

/// Everything the notch can display. Sent over the socket as one JSON object per line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A generic message, e.g. from a script: `arnotchctl notify "Build" "done"`.
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
        /// The agent's working directory, used to find its terminal.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        cwd: String,
        #[serde(flatten)]
        phase: AgentPhase,
    },
    /// Ask the notch to show its settings window.
    OpenSettings,
    /// Open the island, optionally on a tab ("activity", "terminals").
    Open {
        #[serde(default)]
        tab: Option<String>,
    },
    /// Open the island on a tab, or close it if it already shows that tab
    /// (for a keyboard shortcut).
    Toggle {
        #[serde(default)]
        tab: Option<String>,
    },
    /// Close the island.
    Close,
    /// Put files (absolute paths) on the shelf.
    Shelve { paths: Vec<String> },
    /// Share a file (absolute path) with the service chosen in the settings.
    Share { path: String },
    /// Answer the most recent pending permission request.
    Answer { decision: Decision },
    /// The open terminals, published by the notch itself (never accepted from clients).
    #[serde(skip_deserializing)]
    Terminals { terminals: Vec<Terminal> },
    /// Progress of a file being shared; published by the notch itself.
    #[serde(skip_deserializing)]
    Upload {
        id: u32,
        /// File name, for display.
        file: String,
        /// From 0.0 to 1.0.
        progress: f32,
        state: UploadState,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Part of Claude's answer in the island's chat; published by the notch itself.
    #[serde(skip_deserializing)]
    Chat {
        /// The message being answered, so a stopped answer's last pieces can be told apart.
        turn: u32,
        state: ChatState,
        /// The next piece of the answer (`streaming`), or what went wrong (`failed`).
        #[serde(skip_serializing_if = "String::is_empty")]
        text: String,
    },
    /// Volume or brightness changed (0.0 to 1.0); published by the notch itself.
    #[serde(skip_deserializing)]
    Level {
        kind: crate::system::Level,
        value: f32,
        #[serde(default)]
        muted: bool,
    },
    /// The battery's state; published by the notch itself.
    #[serde(skip_deserializing)]
    Battery { battery: crate::system::Battery },
    /// What's playing, `None` when no player has a track; published by the notch itself.
    #[serde(skip_deserializing)]
    Media {
        player: Option<crate::media::Player>,
    },
    /// A permission request was answered or abandoned; published by the notch itself.
    #[serde(skip_deserializing)]
    PermissionClosed { request_id: String },
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
    /// The agent asks to run a tool. The client waits on its connection for one line
    /// with a [`Decision`].
    Permission {
        request_id: String,
        tool: String,
        detail: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<Diff>,
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

/// The user's answer to an [`AgentPhase::Permission`] request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
    /// Let the agent ask in its terminal as usual.
    Ask,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadState {
    Uploading,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatState {
    Streaming,
    Done,
    Failed,
    Stopped,
}
