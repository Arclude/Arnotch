//! The QML-facing side of the notch: `NotchBridge` re-emits every IPC event
//! as a JSON string so QML can react to it.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use arnotch_core::event::VoiceState;
use arnotch_core::event::{ChatState, UploadState};
use arnotch_core::i18n::{self, tr};
use arnotch_core::terminals::{self, Terminal};
use arnotch_core::{
    Decision, Event, chat, clipboard, control, focus, ipc, media, route, share, speech, voice,
};
use cxx_qt::Threading;
use cxx_qt_lib::QString;
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

        /// Answers a pending plan approval or question: a JSON [`Reply`]
        /// (`{"decision": "allow", "answers": {...}, "mode": "...", "message": "..."}`).
        #[qinvokable]
        #[cxx_name = "answerPermissionWith"]
        fn answer_permission_with(
            self: &NotchBridge,
            request_id: &QString,
            reply: &QString,
        ) -> bool;

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

        /// Sends a message to Claude in the island's chat and returns its turn number.
        /// The answer arrives as `chat` events with that turn; with `speak`, it is also read
        /// aloud as it is written (`speech` events tell when).
        #[qinvokable]
        #[cxx_name = "askClaude"]
        fn ask_claude(
            self: &NotchBridge,
            prompt: &QString,
            attachments: &QString,
            web: bool,
            files: bool,
            control: bool,
            edit: bool,
            speak: bool,
            model: &QString,
            vault: &QString,
        ) -> u32;

        /// Stops reading the answer aloud.
        #[qinvokable]
        #[cxx_name = "stopSpeaking"]
        fn stop_speaking(self: &NotchBridge);

        /// Stops the answer being written.
        #[qinvokable]
        #[cxx_name = "stopChat"]
        fn stop_chat(self: &NotchBridge);

        /// Sends "play_pause", "next" or "previous" to a media player.
        #[qinvokable]
        #[cxx_name = "mediaCommand"]
        fn media_command(self: &NotchBridge, bus_name: &QString, command: &QString);

        /// The calendar's iCalendar address (empty: no calendar). Meetings arrive as
        /// `calendar` events.
        #[qinvokable]
        #[cxx_name = "setCalendarUrl"]
        fn set_calendar_url(self: &NotchBridge, url: &QString);

        /// The city for the weather (empty: no weather). It arrives as `weather` events.
        #[qinvokable]
        #[cxx_name = "setWeatherCity"]
        fn set_weather_city(self: &NotchBridge, city: &QString);

        /// Keeps the desktop's notifications quiet (Do Not Disturb), or lets them through again.
        #[qinvokable]
        #[cxx_name = "setDoNotDisturb"]
        fn set_do_not_disturb(self: &NotchBridge, on: bool);

        /// Makes the phone called `device` ring, to find it.
        #[qinvokable]
        #[cxx_name = "ringPhone"]
        fn ring_phone(self: &NotchBridge, device: &QString);

        /// Plays a sound of the desktop's sound theme, e.g. "alarm-clock-elapsed".
        #[qinvokable]
        #[cxx_name = "playSound"]
        fn play_sound(self: &NotchBridge, name: &QString);

        /// Shows a desktop notification for a few seconds; one with the same `tag` replaces it.
        #[qinvokable]
        #[cxx_name = "notifyDesktop"]
        fn notify_desktop(
            self: &NotchBridge,
            tag: &QString,
            summary: &QString,
            body: &QString,
            icon: &QString,
        );

        /// Puts text back on the clipboard.
        #[qinvokable]
        #[cxx_name = "copyText"]
        fn copy_text(self: &NotchBridge, text: &QString);

        /// Starts a new conversation; Claude forgets the previous one.
        #[qinvokable]
        #[cxx_name = "newChat"]
        fn new_chat(self: &NotchBridge);

        /// Starts listening for a question to Claude. What was said arrives as `voice`
        /// events: `listening` with the level, `transcribing`, then `done` with the text.
        #[qinvokable]
        #[cxx_name = "startListening"]
        fn start_listening(self: &NotchBridge);

        /// Done talking: turns what was said into text.
        #[qinvokable]
        #[cxx_name = "stopListening"]
        fn stop_listening(self: &NotchBridge);

        /// Forgets what is being said, or the text it is being turned into.
        #[qinvokable]
        #[cxx_name = "cancelListening"]
        fn cancel_listening(self: &NotchBridge);

        /// Listens for `word` ("Arnotch") on the microphone, and wakes the island when it is
        /// said (`wake` events); empty stops listening.
        #[qinvokable]
        #[cxx_name = "setWakeWord"]
        fn set_wake_word(self: &NotchBridge, word: &QString);

        /// Opens the file manager on the folder of a file, with the file selected.
        #[qinvokable]
        #[cxx_name = "showInFolder"]
        fn show_in_folder(self: &NotchBridge, path: &QString);
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

/// Sent, unseen, when an answer didn't get the job done and a stronger model tries again.
const RETRY_PROMPT: &str = "Your last answer didn't get it done: the last step failed or \
you couldn't do it. Try again, another way if need be, and answer briefly without \
apologizing.";

/// The island's conversation with Claude, shared by every island window.
struct Chat {
    session_id: String,
    turn: u32,
    /// The `claude` process writing the current answer.
    pid: Option<u32>,
    /// The model of the last message, which a short follow-up keeps.
    model: Option<route::Model>,
}

static CHAT: Mutex<Option<Chat>> = Mutex::new(None);

fn with_chat<T>(f: impl FnOnce(&mut Chat) -> T) -> T {
    let mut chat = CHAT.lock().unwrap();
    f(chat.get_or_insert_with(|| Chat {
        session_id: chat::new_session_id(),
        turn: 0,
        pid: None,
        model: None,
    }))
}

/// What's playing, replayed to bridges created after it was published.
static MEDIA: Mutex<Option<Event>> = Mutex::new(None);
/// The upcoming meetings, likewise.
static CALENDAR: Mutex<Option<Event>> = Mutex::new(None);

pub fn publish_calendar(meetings: Vec<arnotch_core::calendar::Meeting>) {
    let event = Event::Calendar { meetings };
    *CALENDAR.lock().unwrap() = Some(event.clone());
    _ = event_bus().send(event);
}

/// Tells the calendar thread about a new address.
pub static CALENDAR_URL: OnceLock<std::sync::mpsc::Sender<String>> = OnceLock::new();

/// Tells the weather thread about a new city.
pub static WEATHER_CITY: OnceLock<std::sync::mpsc::Sender<String>> = OnceLock::new();
/// The weather, replayed to bridges created after it was published.
static WEATHER: Mutex<Option<Event>> = Mutex::new(None);

pub fn publish_weather(weather: Option<arnotch_core::weather::Weather>) {
    let event = Event::Weather { weather };
    *WEATHER.lock().unwrap() = Some(event.clone());
    _ = event_bus().send(event);
}

/// The transfers in progress, likewise.
static JOBS: Mutex<Option<Event>> = Mutex::new(None);

pub fn publish_jobs(jobs: Vec<arnotch_core::jobs::Job>) {
    let event = Event::Jobs { jobs };
    *JOBS.lock().unwrap() = Some(event.clone());
    _ = event_bus().send(event);
}

/// The phone's latest battery state (or that it went away), likewise.
static PHONE: Mutex<Option<Event>> = Mutex::new(None);

pub fn publish_phone(event: arnotch_core::phone::PhoneEvent) {
    use arnotch_core::phone::PhoneEvent;
    let event = Event::Phone(event);
    if let Event::Phone(PhoneEvent::Battery { .. } | PhoneEvent::Gone { .. }) = event {
        *PHONE.lock().unwrap() = Some(event.clone());
    }
    _ = event_bus().send(event);
}

/// Who uses the microphone, camera and screen, as PipeWire and `/dev/video*` tell.
static PRIVACY: Mutex<(arnotch_core::privacy::InUse, arnotch_core::privacy::InUse)> = Mutex::new((
    arnotch_core::privacy::InUse::NONE,
    arnotch_core::privacy::InUse::NONE,
));
/// Their union, replayed to bridges created after it was published.
static PRIVACY_EVENT: Mutex<Option<Event>> = Mutex::new(None);

pub fn publish_pipewire(in_use: arnotch_core::privacy::InUse) {
    let both = {
        let mut privacy = PRIVACY.lock().unwrap();
        privacy.0 = in_use;
        privacy.0.merge(&privacy.1)
    };
    publish_privacy(both);
}

pub fn publish_cameras(apps: Vec<String>) {
    let both = {
        let mut privacy = PRIVACY.lock().unwrap();
        privacy.1.camera = apps;
        privacy.0.merge(&privacy.1)
    };
    publish_privacy(both);
}

fn publish_privacy(in_use: arnotch_core::privacy::InUse) {
    let event = Event::Privacy(in_use);
    let mut last = PRIVACY_EVENT.lock().unwrap();
    if last.as_ref() != Some(&event) {
        *last = Some(event.clone());
        _ = event_bus().send(event);
    }
}

/// The question being listened to: where to send Stop or Cancel, and which one it is
/// (a cancelled one's late transcript is dropped).
static LISTENING: Mutex<Option<(u32, std::sync::mpsc::Sender<voice::Control>)>> = Mutex::new(None);
static LISTEN_ROUND: AtomicU32 = AtomicU32::new(0);
/// Held while a round's voice event is checked and sent, and while a round is cancelled:
/// nothing of a cancelled round is sent after its "cancelled".
static VOICE_EVENTS: Mutex<()> = Mutex::new(());

/// Sends a voice event of the round, unless it was cancelled meanwhile.
fn send_voice(round: u32, state: VoiceState, level: f32, text: String) {
    let _sending = VOICE_EVENTS.lock().unwrap();
    if LISTEN_ROUND.load(Ordering::Relaxed) == round {
        _ = event_bus().send(Event::Voice { state, level, text });
    }
}
/// A question is being listened to or turned into text.
static HEARING: AtomicBool = AtomicBool::new(false);

/// Clears `HEARING` when the question is done with, however it ends.
struct Hearing;

impl Drop for Hearing {
    fn drop(&mut self) {
        HEARING.store(false, Ordering::Relaxed);
    }
}

/// The wake word listened for, and the switch that stops its listener.
static WAKE: Mutex<Option<(String, std::sync::Arc<AtomicBool>)>> = Mutex::new(None);

/// Listens for the wake word until `on` is turned off.
fn listen_for_wake_word(word: String, on: std::sync::Arc<AtomicBool>) {
    let Some(model) = voice::find_wake_model() else {
        _ = event_bus().send(Event::Notify {
            title: tr("Can't listen for the wake word").to_owned(),
            body: tr("No speech model: run arnotchctl voice-model base").to_owned(),
        });
        return;
    };
    tracing::info!(
        "listening for the wake word {word:?} with {}",
        model.display()
    );
    // arnotch doesn't wake itself: not while it listens for a question or speaks.
    let paused = || HEARING.load(Ordering::Relaxed) || SPEAKING.lock().unwrap().is_some();
    let mut failures = 0;
    while on.load(Ordering::Relaxed) {
        let result = arnotch_core::wake::listen(
            &word,
            &model,
            || on.load(Ordering::Relaxed),
            paused,
            |question| _ = event_bus().send(Event::Wake { question }),
        );
        let Err(err) = result else { break };
        // The microphone went away (a headset unplugged...): try again a little later.
        failures += 1;
        tracing::warn!("wake word: {err:#}");
        if failures == 5 {
            _ = event_bus().send(Event::Notify {
                title: tr("Can't listen for the wake word").to_owned(),
                body: format!("{err:#}"),
            });
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// The answer being read aloud: its Piper and pw-play processes.
static SPEAKING: Mutex<Option<[u32; 2]>> = Mutex::new(None);
/// Bumped to stop reading: a reader of an earlier round stops at its next sentence.
static SPEECH_ROUND: AtomicU32 = AtomicU32::new(0);

fn stop_speaking() {
    SPEECH_ROUND.fetch_add(1, Ordering::Relaxed);
    if let Some(pids) = SPEAKING.lock().unwrap().take() {
        speech::silence(pids);
    }
}

/// Reads the sentences aloud as they arrive, until they end or reading is stopped, then
/// says it is done.
fn speak_answer(turn: u32, sentences: std::sync::mpsc::Receiver<String>) {
    let speaking = |speaking| _ = event_bus().send(Event::Speech { turn, speaking });
    let round = SPEECH_ROUND.load(Ordering::Relaxed);
    let current = || SPEECH_ROUND.load(Ordering::Relaxed) == round;
    let mut speaker: Option<speech::Speaker> = None;
    for sentence in sentences {
        if !current() {
            break;
        }
        if speaker.is_none() {
            // Piper starts with the first sentence, so a failed answer costs nothing.
            match speech::Speaker::new() {
                Ok(started) => {
                    *SPEAKING.lock().unwrap() = Some(started.pids());
                    speaking(true);
                    speaker = Some(started);
                }
                Err(err) => {
                    tracing::warn!("speech: {err:#}");
                    _ = event_bus().send(Event::Notify {
                        title: tr("Can't read the answer aloud").to_owned(),
                        body: format!("{err:#}"),
                    });
                    return speaking(false);
                }
            }
        }
        if let Some(speaker) = speaker.as_mut()
            && speaker.say(&sentence).is_err()
        {
            break;
        }
    }
    let Some(speaker) = speaker else {
        return speaking(false);
    };
    let pids = speaker.pids();
    if !current() {
        speech::silence(pids);
    }
    speaker.finish();
    {
        let mut current = SPEAKING.lock().unwrap();
        if *current == Some(pids) {
            *current = None;
        }
    }
    speaking(false);
}

/// Do Not Disturb while focusing, ended by dropping it.
static QUIET: Mutex<Option<arnotch_core::system::NotificationInhibitor>> = Mutex::new(None);

/// The clipboard history, likewise.
static CLIPBOARD: Mutex<Option<Event>> = Mutex::new(None);

pub fn publish_clipboard(items: Vec<arnotch_core::clipboard::Entry>) {
    let event = Event::Clipboard { items };
    *CLIPBOARD.lock().unwrap() = Some(event.clone());
    _ = event_bus().send(event);
}

pub fn publish_media(player: Option<media::Player>) {
    let event = Event::Media { player };
    *MEDIA.lock().unwrap() = Some(event.clone());
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
        ipc::answer(&request_id.to_string(), decision.into())
    }

    pub fn answer_permission_with(&self, request_id: &QString, reply: &QString) -> bool {
        match serde_json::from_str::<arnotch_core::Reply>(&reply.to_string()) {
            Ok(reply) => ipc::answer(&request_id.to_string(), reply),
            Err(err) => {
                tracing::warn!("invalid reply from the island: {err}");
                false
            }
        }
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

impl qobject::NotchBridge {
    /// `attachments` holds one path per line; `web` and `files` turn on Claude's
    /// read-only web and file tools, `control` its commands to work the computer, `edit`
    /// changing files (each change asked about in the island). `model`
    /// is "auto" (chosen for each message) or the model to use; `vault` the Obsidian vault
    /// research is written to, empty for none.
    pub fn ask_claude(
        &self,
        prompt: &QString,
        attachments: &QString,
        web: bool,
        files: bool,
        control: bool,
        edit: bool,
        speak: bool,
        model: &QString,
        vault: &QString,
    ) -> u32 {
        // A new question silences the previous answer.
        stop_speaking();
        let mut request = chat::Request {
            prompt: prompt.to_string(),
            attachments: attachments
                .to_string()
                .lines()
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .collect(),
            web,
            files,
            control,
            edit,
            route: None,
            vault: Some(vault.to_string())
                .filter(|v| !v.is_empty())
                .map(|v| match v.strip_prefix("~/") {
                    Some(rest) => chat::home().join(rest),
                    None => PathBuf::from(v),
                })
                .filter(|v| v.is_dir()),
        };
        // One answer at a time: a new message replaces the one being written.
        let (session_id, turn, route) = with_chat(|chat| {
            if let Some(pid) = chat.pid.take() {
                chat::stop(pid);
            }
            chat.turn += 1;
            let route = route::pick(
                &model.to_string(),
                &request.prompt,
                &request.attachments,
                chat.model,
            );
            chat.model = Some(route.model);
            (chat.session_id.clone(), chat.turn, route)
        });
        request.route = Some(route);
        let send = move |state: ChatState, text: String| {
            _ = event_bus().send(Event::Chat { turn, state, text });
        };
        send(ChatState::Model, route.model.name().to_owned());
        let reader = speak.then(|| {
            let (say, said) = std::sync::mpsc::channel::<String>();
            std::thread::spawn(move || speak_answer(turn, said));
            say
        });

        std::thread::spawn(move || {
            let mut request = request;
            let mut route = route;
            let result = loop {
                let mut said = String::new();
                let attempt =
                    chat::ask(&session_id, chat::saved(&session_id), &request).and_then(|answer| {
                        // Stopped before it started: don't let it run.
                        let current = with_chat(|chat| {
                            let current = chat.turn == turn && chat.session_id == session_id;
                            if current {
                                chat.pid = Some(answer.pid());
                            }
                            current
                        });
                        if !current {
                            chat::stop(answer.pid());
                        }
                        let mut sentences = speech::Sentences::default();
                        let streamed = answer.stream(|piece| match piece {
                            chat::Piece::Text(text) => {
                                if let Some(reader) = &reader {
                                    for sentence in sentences.push(text) {
                                        _ = reader.send(sentence);
                                    }
                                }
                                said.push_str(text);
                                send(ChatState::Streaming, text.to_owned())
                            }
                            chat::Piece::Tool(name) => send(ChatState::Tool, name.to_owned()),
                            chat::Piece::Change(diff) => send(
                                ChatState::Change,
                                serde_json::to_string(diff).unwrap_or_default(),
                            ),
                        });
                        if let Some(reader) = &reader {
                            for sentence in sentences.finish() {
                                _ = reader.send(sentence);
                            }
                        }
                        streamed
                    });
                let outcome = match attempt {
                    Ok(outcome) => outcome,
                    Err(err) => break Err(err),
                };
                let current = with_chat(|chat| {
                    chat.turn == turn && chat.session_id == session_id && chat.pid.is_some()
                });
                // It didn't get it done: once more, in the same conversation, a model up.
                let Some(stronger) =
                    route::escalate(route, outcome.last_tool_failed, &said).filter(|_| current)
                else {
                    break Ok(());
                };
                tracing::info!("chat: {} → {}", route.model.name(), stronger.model.name());
                send(
                    ChatState::Model,
                    format!("{} → {}", route.model.name(), stronger.model.name()),
                );
                send(ChatState::Streaming, "\n\n".to_owned());
                with_chat(|chat| chat.model = Some(stronger.model));
                route = stronger;
                request.prompt = RETRY_PROMPT.to_owned();
                request.attachments.clear();
                request.route = Some(stronger);
            };
            // The reader ends once it has said everything sent.
            drop(reader);
            // Still the current answer, or was it stopped or replaced?
            let current = with_chat(|chat| {
                let current =
                    chat.turn == turn && chat.session_id == session_id && chat.pid.is_some();
                if current {
                    chat.pid = None;
                }
                current
            });
            if !current || result.is_err() {
                stop_speaking();
            }
            match result {
                _ if !current => send(ChatState::Stopped, String::new()),
                Ok(()) => send(ChatState::Done, String::new()),
                Err(err) => {
                    tracing::warn!("chat: {err:#}");
                    send(ChatState::Failed, format!("{err:#}"));
                }
            }
        });
        turn
    }

    pub fn media_command(&self, bus_name: &QString, command: &QString) {
        let command = match command.to_string().as_str() {
            "next" => media::Command::Next,
            "previous" => media::Command::Previous,
            _ => media::Command::PlayPause,
        };
        let bus_name = bus_name.to_string();
        std::thread::spawn(move || {
            if let Err(err) = media::command(&bus_name, command) {
                tracing::warn!("media command for {bus_name}: {err:#}");
            }
        });
    }

    pub fn set_calendar_url(&self, url: &QString) {
        if let Some(tx) = CALENDAR_URL.get() {
            _ = tx.send(url.to_string());
        }
    }

    pub fn set_weather_city(&self, city: &QString) {
        if let Some(tx) = WEATHER_CITY.get() {
            _ = tx.send(city.to_string());
        }
    }

    pub fn set_do_not_disturb(&self, on: bool) {
        let mut quiet = QUIET.lock().unwrap();
        if !on {
            *quiet = None;
        } else if quiet.is_none() {
            match arnotch_core::system::NotificationInhibitor::new(tr("Focusing")) {
                Ok(inhibitor) => *quiet = Some(inhibitor),
                Err(err) => tracing::warn!("cannot turn on Do Not Disturb: {err:#}"),
            }
        }
    }

    pub fn ring_phone(&self, device: &QString) {
        let device = device.to_string();
        std::thread::spawn(move || {
            if let Err(err) = arnotch_core::phone::ring(&device) {
                _ = event_bus().send(Event::Notify {
                    title: tr("Can't ring the phone").to_owned(),
                    body: format!("{err:#}"),
                });
            }
        });
    }

    pub fn play_sound(&self, name: &QString) {
        arnotch_core::system::play_sound(&name.to_string());
    }

    pub fn notify_desktop(&self, tag: &QString, summary: &QString, body: &QString, icon: &QString) {
        let (tag, summary, body, icon) = (
            tag.to_string(),
            summary.to_string(),
            body.to_string(),
            icon.to_string(),
        );
        std::thread::spawn(move || {
            if let Err(err) = arnotch_core::system::notify(&tag, &summary, &body, &icon, 4000) {
                tracing::warn!("cannot notify: {err:#}");
            }
        });
    }

    pub fn copy_text(&self, text: &QString) {
        let text = text.to_string();
        std::thread::spawn(move || {
            if let Err(err) = clipboard::copy(&text) {
                tracing::warn!("cannot copy: {err:#}");
            }
        });
    }

    pub fn stop_speaking(&self) {
        stop_speaking();
    }

    pub fn stop_chat(&self) {
        stop_speaking();
        with_chat(|chat| {
            if let Some(pid) = chat.pid.take() {
                chat::stop(pid);
            }
            // Also stops an answer whose process is still starting.
            chat.turn += 1;
        });
    }

    pub fn start_listening(&self) {
        // Claude stops talking while the user does.
        stop_speaking();
        let (control, controls) = std::sync::mpsc::channel();
        let round = {
            let mut listening = LISTENING.lock().unwrap();
            if listening.is_some() {
                return;
            }
            let round = LISTEN_ROUND.fetch_add(1, Ordering::Relaxed) + 1;
            *listening = Some((round, control));
            round
        };
        // Only while still this round: not cancelled meanwhile.
        let send = move |state: VoiceState, level: f32, text: String| {
            send_voice(round, state, level, text)
        };
        send(VoiceState::Listening, 0.0, String::new());
        HEARING.store(true, Ordering::Relaxed);

        std::thread::spawn(move || {
            let _hearing = Hearing;
            let recorded = voice::record(&controls, |level| {
                send(VoiceState::Listening, level, String::new())
            });
            // From here on, Stop has nothing more to stop; Cancel still drops the text.
            LISTENING.lock().unwrap().take();
            // A cancelled round says nothing more: cancelling said it.
            let samples = match recorded {
                Ok(Some(samples)) => samples,
                Ok(None) => return send(VoiceState::Cancelled, 0.0, String::new()),
                Err(err) => return send(VoiceState::Failed, 0.0, format!("{err:#}")),
            };
            send(VoiceState::Transcribing, 0.0, String::new());
            match voice::transcribe(&samples) {
                Ok(text) if text.is_empty() => send(VoiceState::Cancelled, 0.0, String::new()),
                Ok(text) => send(VoiceState::Done, 0.0, text),
                Err(err) => {
                    tracing::warn!("voice: {err:#}");
                    send(VoiceState::Failed, 0.0, format!("{err:#}"));
                }
            }
        });
    }

    pub fn stop_listening(&self) {
        if let Some((_, control)) = LISTENING.lock().unwrap().as_ref() {
            _ = control.send(voice::Control::Stop);
        }
    }

    pub fn cancel_listening(&self) {
        if let Some((_, control)) = LISTENING.lock().unwrap().take() {
            _ = control.send(voice::Control::Cancel);
        }
        // A transcript on its way is dropped too. "cancelled" comes after the round's last
        // level, which would otherwise show the island listening again, and for good.
        let _sending = VOICE_EVENTS.lock().unwrap();
        LISTEN_ROUND.fetch_add(1, Ordering::Relaxed);
        if HEARING.load(Ordering::Relaxed) {
            _ = event_bus().send(Event::Voice {
                state: VoiceState::Cancelled,
                level: 0.0,
                text: String::new(),
            });
        }
    }

    pub fn set_wake_word(&self, word: &QString) {
        let word = word.to_string().trim().to_owned();
        let mut wake = WAKE.lock().unwrap();
        if wake
            .as_ref()
            .is_some_and(|(listening, _)| *listening == word)
        {
            return;
        }
        if let Some((_, on)) = wake.take() {
            on.store(false, Ordering::Relaxed);
        }
        if word.is_empty() {
            return;
        }
        let on = std::sync::Arc::new(AtomicBool::new(true));
        *wake = Some((word.clone(), on.clone()));
        std::thread::spawn(move || listen_for_wake_word(word, on));
    }

    pub fn show_in_folder(&self, path: &QString) {
        let path = path.to_string();
        std::thread::spawn(move || {
            if let Err(err) = arnotch_core::system::show_in_folder(&path) {
                tracing::warn!("cannot show {path} in its folder: {err:#}");
            }
        });
    }

    pub fn new_chat(&self) {
        stop_speaking();
        with_chat(|chat| {
            if let Some(pid) = chat.pid.take() {
                chat::stop(pid);
            }
            chat.session_id = chat::new_session_id();
            chat.model = None;
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
        let replay = [
            TERMINALS.lock().unwrap().clone(),
            MEDIA.lock().unwrap().clone(),
            CLIPBOARD.lock().unwrap().clone(),
            CALENDAR.lock().unwrap().clone(),
            WEATHER.lock().unwrap().clone(),
            JOBS.lock().unwrap().clone(),
            PHONE.lock().unwrap().clone(),
            PRIVACY_EVENT.lock().unwrap().clone(),
        ];
        for event in replay.into_iter().flatten() {
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
