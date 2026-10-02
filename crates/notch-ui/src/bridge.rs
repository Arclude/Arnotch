//! The QML-facing side of the notch: `NotchBridge` re-emits every IPC event
//! as a JSON string so QML can react to it.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use notch_core::terminals::{self, Terminal};
use notch_core::{Decision, Event, focus, ipc};
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

        /// Tells the notch whether a terminal list is on screen, to rescan more often.
        #[qinvokable]
        #[cxx_name = "setTerminalsVisible"]
        fn set_terminals_visible(self: &NotchBridge, visible: bool);

        /// Brings the terminal emulator's window to the front.
        #[qinvokable]
        #[cxx_name = "focusTerminal"]
        fn focus_terminal(self: &NotchBridge, app_pid: u32);
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

    pub fn set_terminals_visible(&self, visible: bool) {
        TERMINALS_VISIBLE.store(visible, Ordering::Relaxed);
    }

    pub fn focus_terminal(&self, app_pid: u32) {
        // Talks to the compositor through other processes; keep the UI thread free.
        std::thread::spawn(move || {
            if let Err(err) = focus::focus_window(app_pid) {
                tracing::warn!("cannot focus window of {app_pid}: {err:#}");
                _ = event_bus().send(Event::Notify {
                    title: "Can't switch to the terminal".into(),
                    body: err.to_string(),
                });
            }
        });
    }
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
