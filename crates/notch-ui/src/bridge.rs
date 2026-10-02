//! The QML-facing side of the notch: `NotchBridge` re-emits every IPC event
//! as a JSON string so QML can react to it.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use notch_core::event::UploadState;
use notch_core::i18n::{self, tr};
use notch_core::terminals::{self, Terminal};
use notch_core::{Decision, Event, clipboard, control, focus, ipc, share};
use tokio::sync::broadcast;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type NotchBridge = super::NotchBridgeRust;
    }

    unsafe extern "RustQt" {
        /// Emitted on the Qt thread for every event received over IPC.
        #[qsignal]
        #[cxx_name = "eventReceived"]
        fn event_received(self: Pin<&mut NotchBridge>, json: QString);

        /// Sends Ctrl+C to the command running in the terminal whose shell is `shell_pid`.
        #[qinvokable]
        #[cxx_name = "interruptCommand"]
        fn interrupt_command(self: &NotchBridge, shell_pid: u32) -> bool;

        /// Answers a pending permission request: "allow", "deny" or "ask".
        #[qinvokable]
        #[cxx_name = "answerPermission"]
        fn answer_permission(self: &NotchBridge, request_id: &QString, decision: &QString) -> bool;

        /// Uploads a file to the chosen sharing service and copies the link.
        /// Progress arrives as `upload` events.
        #[qinvokable]
        #[cxx_name = "shareFile"]
        fn share_file(self: &NotchBridge, path: &QString, service: &QString, command: &QString);

        /// Mentions files (one path per line) in the Claude session running in a
        /// terminal, without submitting, then switches to that terminal.
        #[qinvokable]
        #[cxx_name = "sendFilesToClaude"]
        fn send_files_to_claude(self: &NotchBridge, terminal_id: &QString, paths: &QString);

        /// `{"exists": bool, "size": bytes, "dir": bool}` as JSON.
        #[qinvokable]
        #[cxx_name = "fileInfo"]
        fn file_info(self: &NotchBridge, path: &QString) -> QString;

        /// Tells the notch whether a terminal list is on screen, to rescan more often.
        #[qinvokable]
        #[cxx_name = "setTerminalsVisible"]
        fn set_terminals_visible(self: &NotchBridge, visible: bool);

        /// Brings a terminal's window to the front and selects its tab.
        #[qinvokable]
        #[cxx_name = "focusTerminal"]
        fn focus_terminal(self: &NotchBridge, id: &QString);

        /// Types `text` into a terminal and presses Enter.
        #[qinvokable]
        #[cxx_name = "sendText"]
        fn send_text(self: &NotchBridge, id: &QString, text: &QString);
    }

    impl cxx_qt::Threading for NotchBridge {}
    impl cxx_qt::Initialize for NotchBridge {}
}

#[derive(Default)]
pub struct NotchBridgeRust;

static EVENTS: OnceLock<broadcast::Sender<Event>> = OnceLock::new();

/// The process-wide event bus. The IPC server publishes to it, every `NotchBridge` subscribes.
pub fn event_bus() -> broadcast::Sender<Event> {
    EVENTS.get_or_init(|| broadcast::channel(256).0).clone()
}

/// Whether some island currently shows the terminal list.
pub static TERMINALS_VISIBLE: AtomicBool = AtomicBool::new(false);

/// The last terminal list, replayed to bridges created after it was published.
static TERMINALS: Mutex<Option<Event>> = Mutex::new(None);

pub fn publish_terminals(terminals: Vec<Terminal>) {
    let event = Event::Terminals { terminals };
    *TERMINALS.lock().unwrap() = Some(event.clone());
    _ = event_bus().send(event);
}

fn emit(bridge: Pin<&mut qobject::NotchBridge>, event: &Event) {
    let json = serde_json::to_string(event).expect("event is always serializable");
    bridge.event_received(QString::from(&json));
}

impl qobject::NotchBridge {
    pub fn interrupt_command(&self, shell_pid: u32) -> bool {
        match terminals::interrupt(shell_pid) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!("cannot interrupt the command of shell {shell_pid}: {err}");
                false
            }
        }
    }

    pub fn answer_permission(&self, request_id: &QString, decision: &QString) -> bool {
        let decision = match decision.to_string().as_str() {
            "allow" => Decision::Allow,
            "deny" => Decision::Deny,
            _ => Decision::Ask,
        };
        ipc::answer(&request_id.to_string(), decision)
    }

    pub fn share_file(&self, path: &QString, service: &QString, command: &QString) {
        static NEXT_ID: AtomicU32 = AtomicU32::new(1);

        let path = PathBuf::from(path.to_string());
        let file = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let Some(service) =
            share::Service::from_setting(&service.to_string(), &command.to_string())
        else {
            _ = event_bus().send(Event::Notify {
                title: tr("Sharing is off").into(),
                body: tr("Choose a sharing service in the settings").into(),
            });
            return;
        };

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let report =
            move |progress: f32, state: UploadState, url: Option<String>, error: Option<String>| {
                _ = event_bus().send(Event::Upload {
                    id,
                    file: file.clone(),
                    progress,
                    state,
                    url,
                    error,
                });
            };
        report(0.0, UploadState::Uploading, None, None);

        std::thread::spawn(move || {
            // curl redraws its bar many times a second; a percent step is enough.
            let mut last = 0.0;
            let result = share::upload(&path, &service, |progress| {
                if progress - last >= 0.01 {
                    last = progress;
                    report(progress, UploadState::Uploading, None, None);
                }
            });
            match result {
                Ok(url) => {
                    if let Err(err) = clipboard::copy(&url) {
                        tracing::warn!("cannot copy the link: {err:#}");
                    }
                    report(1.0, UploadState::Done, Some(url), None);
                }
                Err(err) => report(last, UploadState::Failed, None, Some(format!("{err:#}"))),
            }
        });
    }

    pub fn send_files_to_claude(&self, terminal_id: &QString, paths: &QString) {
        // Claude Code reads files mentioned as @path; quote paths with spaces.
        let mentions: Vec<String> = paths
            .to_string()
            .lines()
            .filter(|p| !p.is_empty())
            .map(|p| {
                if p.contains(' ') {
                    format!("@\"{p}\"")
                } else {
                    format!("@{p}")
                }
            })
            .collect();
        let text = format!("{} ", mentions.join(" "));
        with_terminal(
            terminal_id,
            tr("Can't send the file to Claude"),
            move |terminal| {
                let typed = terminal.can_send
                    && control::send_text(terminal, &text, false)
                        .inspect_err(|err| {
                            tracing::warn!("cannot type into {}: {err:#}", terminal.id)
                        })
                        .is_ok();
                if !typed {
                    // The terminal can't be typed into (e.g. Konsole's setting is off):
                    // hand the mention over through the clipboard instead.
                    clipboard::copy(text.trim_end())?;
                    _ = event_bus().send(Event::Notify {
                        title: tr("File path copied").into(),
                        body: tr("Paste it into Claude with Ctrl+Shift+V").into(),
                    });
                }
                control::focus_tab(terminal).or_else(|_| focus::focus_window(terminal.app_pid))
            },
        );
    }

    pub fn file_info(&self, path: &QString) -> QString {
        let info = match std::fs::metadata(path.to_string()) {
            Ok(meta) => {
                serde_json::json!({ "exists": true, "size": meta.len(), "dir": meta.is_dir() })
            }
            Err(_) => serde_json::json!({ "exists": false, "size": 0, "dir": false }),
        };
        QString::from(&info.to_string())
    }

    pub fn set_terminals_visible(&self, visible: bool) {
        TERMINALS_VISIBLE.store(visible, Ordering::Relaxed);
    }

    pub fn focus_terminal(&self, id: &QString) {
        with_terminal(id, tr("Can't switch to the terminal"), |terminal| {
            control::focus_tab(terminal)
                // A tab we can't select still has a window we can raise.
                .or_else(|_| focus::focus_window(terminal.app_pid))
        });
    }

    pub fn send_text(&self, id: &QString, text: &QString) {
        let text = text.to_string();
        with_terminal(id, tr("Can't type into the terminal"), move |terminal| {
            control::send_text(terminal, &text, true)
        });
    }
}

/// Runs `action` on a background thread (it talks to other processes) for the terminal
/// with this id, reporting failures in the island.
fn with_terminal(
    id: &QString,
    failure: &'static str,
    action: impl FnOnce(&Terminal) -> anyhow::Result<()> + Send + 'static,
) {
    let id = id.to_string();
    let terminal = match TERMINALS.lock().unwrap().as_ref() {
        Some(Event::Terminals { terminals }) => terminals.iter().find(|t| t.id == id).cloned(),
        _ => None,
    };
    std::thread::spawn(move || {
        let result = terminal
            .ok_or_else(|| anyhow::anyhow!(i18n::trf("Terminal {} is gone", &[&id])))
            .and_then(|terminal| action(&terminal));
        if let Err(err) = result {
            tracing::warn!("{failure} ({id}): {err:#}");
            _ = event_bus().send(Event::Notify {
                title: failure.into(),
                body: format!("{err:#}"),
            });
        }
    });
}

impl cxx_qt::Initialize for qobject::NotchBridge {
    fn initialize(self: Pin<&mut Self>) {
        let mut events = event_bus().subscribe();
        let qt_thread = self.qt_thread();

        // Queued: QML connects its handlers only after this object is initialized.
        if let Some(event) = TERMINALS.lock().unwrap().clone() {
            _ = qt_thread.queue(move |bridge| emit(bridge, &event));
        }

        std::thread::spawn(move || {
            loop {
                let event = match events.blocking_recv() {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!("UI fell behind, skipped {skipped} events");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let queued = qt_thread.queue(move |bridge| emit(bridge, &event));
                // The QObject was destroyed, nobody is listening anymore.
                if queued.is_err() {
                    break;
                }
            }
        });
    }
}
