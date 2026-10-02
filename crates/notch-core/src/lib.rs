//! Shared pieces of notch: the event protocol, the IPC socket and
//! adapters that turn third-party payloads (Claude Code hooks, ...) into events.

pub mod claude;
pub mod diff;
pub mod event;
pub mod ipc;

pub use event::{AgentPhase, Event};
