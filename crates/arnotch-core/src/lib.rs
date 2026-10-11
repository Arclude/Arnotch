//! Shared pieces of notch: the event protocol, the IPC socket and
//! adapters that turn third-party payloads (Claude Code hooks, ...) into events.

pub mod agents;
pub mod apps;
pub mod calendar;
pub mod chat;
pub mod claude;
pub mod clipboard;
pub mod control;
pub mod diff;
pub mod event;
pub mod focus;
pub mod i18n;
pub mod ipc;
pub mod jobs;
pub mod mail;
pub mod media;
pub mod phone;
pub mod plans;
pub mod privacy;
pub mod route;
pub mod share;
pub mod speech;
pub mod system;
pub mod terminals;
pub mod voice;
pub mod wake;
pub mod weather;

pub use event::{AgentPhase, Decision, Event, Reply};
