//! Command line client for the notch.
//!
//!   arnotchctl notify <title> [body]   show a message
//!   arnotchctl open [tab]              open the island (tabs: activity, chat, terminals, shelf)
//!   arnotchctl toggle [tab]            open the island on a tab, or close it if it shows that tab
//!   arnotchctl close                   close the island
//!   arnotchctl timer <time> [label]    start a timer: 25m, 1h30m, 90s, 10 (minutes)
//!   arnotchctl timer cancel            cancel it
//!   arnotchctl progress <id> <percent|busy|done|fail|-> [title] [detail]
//!                                    show a script's progress; `-` reads percentages
//!                                    from stdin (curl, rsync, pv, ...) and passes it on
//!   arnotchctl listen [stop|cancel]    ask Claude by voice (again: stop listening)
//!   arnotchctl voice-model [name]      download a speech model (default: small)
//!   arnotchctl speech-voice [name]     download Piper and a voice to read answers aloud
//!   arnotchctl launch <app name>       open an installed app by name
//!   arnotchctl apps                    list installed apps
//!   arnotchctl shelf <file>...         put files on the shelf
//!   arnotchctl share <file>            upload a file and copy its link
//!   arnotchctl screenshot              select a region (Spectacle), then pick where it goes
//!   arnotchctl answer <decision>       answer Claude's latest permission request
//!                                    (allow, deny, ask)
//!   arnotchctl settings                open the settings window
//!   arnotchctl terminals               list open terminals and what runs in them
//!   arnotchctl claude-hook             forward a Claude Code hook payload read from stdin
//!   arnotchctl statusline              Claude Code status line: forwards usage, then runs
//!                                     the previous status line command
//!   arnotchctl send '<json event>'     send a raw event

use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use arnotch_core::event::{ListenAction, ProgressState};
use arnotch_core::i18n::{tr, trf};
use arnotch_core::route::{self, Model};
use arnotch_core::terminals::{self, Terminal};
use arnotch_core::{AgentPhase, Decision, Event, claude, control, ipc};

const USAGE: &str = "usage:
  arnotchctl notify <title> [body]
  arnotchctl open [activity|chat|terminals|shelf|phone]
  arnotchctl toggle [activity|chat|terminals|shelf|phone]
  arnotchctl close
  arnotchctl timer <25m|1h30m|90s|10> [label] | cancel
  arnotchctl focus [25m|50] | stop
  arnotchctl progress <id> <0-100|busy|done|fail|-> [title] [detail]
  arnotchctl listen [stop|cancel]
  arnotchctl voice-model [large-v3-turbo(-q5_0)|medium|small|base|tiny]
  arnotchctl speech-voice [tr_TR-dfki-medium|en_US-lessac-medium|...]
  arnotchctl launch <app name>
  arnotchctl apps
  arnotchctl shelf <file>...
  arnotchctl share <file>
  arnotchctl screenshot
  arnotchctl answer allow|deny|ask
  arnotchctl settings
  arnotchctl terminals
  arnotchctl claude-hook        (reads the hook JSON from stdin)
  arnotchctl send '<json event>'";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match args.as_slice() {
        ["notify", title, rest @ ..] => send(&Event::Notify {
            title: (*title).to_owned(),
            body: rest.join(" "),
        }),
        ["open", rest @ ..] if rest.len() <= 1 => send(&Event::Open {
            tab: rest.first().map(|tab| (*tab).to_owned()),
        }),
        ["toggle", rest @ ..] if rest.len() <= 1 => send(&Event::Toggle {
            tab: rest.first().map(|tab| (*tab).to_owned()),
        }),
        ["close"] => send(&Event::Close),
        ["timer", "cancel"] => send(&Event::TimerCancel),
        ["timer", time, label @ ..] => match arnotch_core::system::parse_duration(time) {
            Some(seconds) if seconds > 0 => send(&Event::Timer {
                seconds,
                label: label.join(" "),
            }),
            _ => {
                eprintln!("arnotchctl: the time must look like 25m, 1h30m, 90s or 10 (minutes)");
                ExitCode::from(2)
            }
        },
        ["focus"] => send(&Event::Focus { minutes: None }),
        ["focus", "stop"] => send(&Event::TimerCancel),
        ["focus", time] => match arnotch_core::system::parse_duration(time) {
            Some(seconds) if seconds >= 60 => send(&Event::Focus {
                minutes: Some(seconds / 60),
            }),
            _ => {
                eprintln!("arnotchctl: the focus time must look like 25m, 1h or 50 (minutes)");
                ExitCode::from(2)
            }
        },
        ["answer", decision] => {
            match serde_json::from_value::<Decision>(serde_json::json!(decision)) {
                Ok(decision) => send(&Event::Answer {
                    reply: decision.into(),
                }),
                Err(_) => {
                    eprintln!("arnotchctl: the answer must be allow, deny or ask");
                    ExitCode::from(2)
                }
            }
        }
        ["progress", id, "-", rest @ ..] => progress_from_stdin(id, rest),
        ["progress", id, value, rest @ ..] => {
            let (state, percent) = match *value {
                "done" => (ProgressState::Done, Some(100.0)),
                "fail" | "failed" => (ProgressState::Failed, None),
                "busy" | "?" => (ProgressState::Running, None),
                number => match number
                    .trim_end_matches('%')
                    .replace(',', ".")
                    .parse::<f32>()
                {
                    Ok(p) if (0.0..=100.0).contains(&p) => (ProgressState::Running, Some(p)),
                    _ => {
                        eprintln!(
                            "arnotchctl: the progress must be 0 to 100, busy, done, fail or -"
                        );
                        return ExitCode::from(2);
                    }
                },
            };
            send(&progress(id, percent, rest, state))
        }
        ["listen"] => send(&Event::Listen {
            action: ListenAction::Toggle,
        }),
        ["listen", action] => match *action {
            "start" => send(&Event::Listen {
                action: ListenAction::Start,
            }),
            "stop" => send(&Event::Listen {
                action: ListenAction::Stop,
            }),
            "cancel" => send(&Event::Listen {
                action: ListenAction::Cancel,
            }),
            _ => {
                eprintln!("{USAGE}");
                ExitCode::from(2)
            }
        },
        ["voice-model"] => voice_model("small"),
        ["voice-model", name] => voice_model(name),
        ["speech-voice"] => speech_voice(arnotch_core::speech::default_voice()),
        ["speech-voice", name] => speech_voice(name),
        ["launch", words @ ..] if !words.is_empty() => launch(&words.join(" ")),
        ["apps"] => {
            for app in arnotch_core::apps::installed() {
                println!("{} ({})", app.name, app.id);
            }
            ExitCode::SUCCESS
        }
        ["shelf", files @ ..] if !files.is_empty() => {
            match files.iter().map(|f| absolute(f)).collect() {
                Ok(paths) => send(&Event::Shelve { paths }),
                Err(code) => code,
            }
        }
        ["share", file] => match absolute(file) {
            Ok(path) => send(&Event::Share { path }),
            Err(code) => code,
        },
        ["settings"] => send(&Event::OpenSettings),
        ["screenshot"] => screenshot(),
        ["terminals"] => {
            let (terminals, _) = arnotch_core::terminals::Scanner::new().scan();
            println!(
                "{}",
                serde_json::to_string_pretty(&terminals).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        ["claude-hook"] => claude_hook(),
        // Started by claude-hook, detached: switches the session's model, then types the
        // prompt again.
        ["route-resend", terminal, model, effort, prompt] => {
            route_resend(terminal, model, effort, prompt)
        }
        ["statusline"] => statusline(),
        ["send", json] => match serde_json::from_str(json) {
            Ok(event) => send(&event),
            Err(err) => {
                eprintln!("arnotchctl: invalid event: {err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Opens the app that best matches the name. Without a clear match, lists what is there,
/// for Claude (or the user) to pick from.
fn launch(query: &str) -> ExitCode {
    use arnotch_core::apps;
    let installed = apps::installed();
    let found = apps::find(&installed, query);
    let Some(best) = found.first() else {
        println!("No app called \"{query}\". Installed apps:");
        for app in &installed {
            println!("  {} ({})", app.name, app.id);
        }
        return ExitCode::FAILURE;
    };
    match apps::launch(best) {
        Ok(()) => {
            println!("Opened {} ({})", best.name, best.id);
            if found.len() > 1 {
                let others: Vec<&str> =
                    found[1..].iter().take(5).map(|a| a.name.as_str()).collect();
                println!("Also matching: {}", others.join(", "));
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("arnotchctl: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn progress(id: &str, percent: Option<f32>, words: &[&str], state: ProgressState) -> Event {
    Event::Progress {
        id: id.to_owned(),
        percent,
        title: words
            .first()
            .map_or_else(|| id.to_owned(), |t| (*t).to_owned()),
        detail: words.get(1..).unwrap_or_default().join(" "),
        state,
    }
}

/// Passes stdin on to stdout, showing the last percentage seen in it, such as curl's,
/// rsync's (--info=progress2) or pv's. Done at the end of the input.
fn progress_from_stdin(id: &str, words: &[&str]) -> ExitCode {
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let mut buffer = [0u8; 8192];
    // Progress bars redraw a line with \r: the text since the last line break is enough.
    let mut line = String::new();
    let mut shown: Option<f32> = None;
    let mut sent_at = std::time::Instant::now() - Duration::from_secs(1);
    _ = ipc::send(&progress(id, None, words, ProgressState::Running));
    loop {
        let read = match stdin.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        // Downstream gone (e.g. `| head`): keep reading for the progress anyway.
        _ = stdout.write_all(&buffer[..read]);
        _ = stdout.flush();
        for chunk in String::from_utf8_lossy(&buffer[..read]).split_inclusive(['\r', '\n']) {
            line.push_str(chunk);
            if let Some(percent) = last_percent(&line) {
                let changed = shown.is_none_or(|p| p.floor() != percent.floor());
                if changed && sent_at.elapsed() >= Duration::from_millis(250) {
                    _ = ipc::send(&progress(id, Some(percent), words, ProgressState::Running));
                    shown = Some(percent);
                    sent_at = std::time::Instant::now();
                }
            }
            if line.ends_with(['\r', '\n']) {
                line.clear();
            }
        }
    }
    _ = ipc::send(&progress(id, Some(100.0), words, ProgressState::Done));
    ExitCode::SUCCESS
}

/// The last "NN%" or "NN.N%" in the text, from 0 to 100.
fn last_percent(text: &str) -> Option<f32> {
    let bytes = text.as_bytes();
    let mut found = None;
    for (at, _) in text.match_indices('%') {
        let start = bytes[..at]
            .iter()
            .rposition(|b| !(b.is_ascii_digit() || *b == b'.' || *b == b','))
            .map_or(0, |i| i + 1);
        if let Ok(percent) = text[start..at].replace(',', ".").parse::<f32>()
            && (0.0..=100.0).contains(&percent)
        {
            found = Some(percent);
        }
    }
    found
}

/// Downloads `url` to `file`, through a partial file so an interrupted download resumes.
fn download(url: &str, file: &std::path::Path) -> bool {
    let partial = file.with_extension("part");
    eprintln!("Downloading {url}");
    let status = std::process::Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--progress-bar",
            "--continue-at",
            "-",
            "--output",
        ])
        .arg(&partial)
        .arg(url)
        .status();
    match status {
        Ok(status) if status.success() => match std::fs::rename(&partial, file) {
            Ok(()) => true,
            Err(err) => {
                eprintln!(
                    "arnotchctl: cannot move {} into place: {err}",
                    file.display()
                );
                false
            }
        },
        Ok(_) => {
            eprintln!("arnotchctl: the download failed; run the same command to resume it");
            false
        }
        Err(err) => {
            eprintln!("arnotchctl: cannot run curl: {err}");
            false
        }
    }
}

/// Downloads Piper (unless there is one) and a voice for reading Claude's answers aloud.
/// Voices are listed at https://huggingface.co/rhasspy/piper-voices.
fn speech_voice(name: &str) -> ExitCode {
    use arnotch_core::speech;
    // "tr_TR-dfki-medium" lives in tr/tr_TR/dfki/medium/
    let parts: Vec<&str> = name.split('-').collect();
    let [locale, speaker, quality] = parts.as_slice() else {
        eprintln!(
            "arnotchctl: a voice looks like tr_TR-dfki-medium (language_COUNTRY-name-quality)"
        );
        return ExitCode::from(2);
    };
    let language = locale.split('_').next().unwrap_or_default();
    let dir = speech::piper_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("arnotchctl: cannot create {}: {err}", dir.display());
        return ExitCode::FAILURE;
    }

    if speech::find_piper().is_none() {
        let archive = dir.join("piper_linux_x86_64.tar.gz");
        if !download(
            "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_x86_64.tar.gz",
            &archive,
        ) {
            return ExitCode::FAILURE;
        }
        let unpacked = std::process::Command::new("tar")
            .arg("xzf")
            .arg(&archive)
            .arg("-C")
            .arg(&dir)
            .status();
        _ = std::fs::remove_file(&archive);
        if !unpacked.is_ok_and(|s| s.success()) {
            eprintln!("arnotchctl: cannot unpack Piper");
            return ExitCode::FAILURE;
        }
    }

    let base = format!(
        "https://huggingface.co/rhasspy/piper-voices/resolve/main/{language}/{locale}/{speaker}/{quality}/{name}"
    );
    for extension in ["onnx.json", "onnx"] {
        if !download(
            &format!("{base}.{extension}"),
            &dir.join(format!("{name}.{extension}")),
        ) {
            return ExitCode::FAILURE;
        }
    }
    eprintln!("Saved the voice {name} in {}", dir.display());
    ExitCode::SUCCESS
}

/// Downloads a whisper.cpp model for asking Claude by voice.
fn voice_model(name: &str) -> ExitCode {
    use arnotch_core::voice;
    if !voice::MODELS.contains(&name) {
        eprintln!(
            "arnotchctl: models are {} (bigger ones understand better, smaller ones answer faster)",
            voice::MODELS.join(", ")
        );
        return ExitCode::from(2);
    }
    let dir = voice::models_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("arnotchctl: cannot create {}: {err}", dir.display());
        return ExitCode::FAILURE;
    }
    let file = dir.join(voice::model_file(name));
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        voice::model_file(name)
    );
    if !download(&url, &file) {
        return ExitCode::FAILURE;
    }
    // The best model present is used: tell when it isn't this one.
    match voice::find_model() {
        Some(used) if used != file => eprintln!(
            "Saved {}; {} is used, as it understands better (delete it to use this one)",
            file.display(),
            used.display()
        ),
        _ => eprintln!("Saved {}", file.display()),
    }
    ExitCode::SUCCESS
}

/// The notch runs elsewhere, so paths must not depend on this shell's directory.
fn absolute(file: &str) -> Result<String, ExitCode> {
    std::fs::canonicalize(file)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|err| {
            eprintln!("arnotchctl: {file}: {err}");
            ExitCode::FAILURE
        })
}

fn send(event: &Event) -> ExitCode {
    match ipc::send(event) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "arnotchctl: cannot reach arnotch at {}: {err}",
                ipc::socket_path().display()
            );
            ExitCode::FAILURE
        }
    }
}

/// Always succeeds: a missing or broken notch must never block Claude Code.
fn claude_hook() -> ExitCode {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return ExitCode::SUCCESS;
    }
    // A prompt for another model: Claude Code doesn't get it now, but again once the
    // session has switched.
    if let Some(output) = route_prompt(&input) {
        println!("{output}");
        return ExitCode::SUCCESS;
    }
    let Ok(Some(event)) = claude::event_from_hook(&input) else {
        return ExitCode::SUCCESS;
    };

    if let Event::Agent {
        phase: AgentPhase::Permission { .. },
        ..
    } = event
    {
        // Wait for the user to answer in the island. Without an answer, print nothing:
        // Claude Code then asks in the terminal as usual.
        let timeout = if claude::needs_input(&input) {
            prompt_timeout()
        } else {
            permission_timeout()
        };
        if let Ok(Some(reply)) = ipc::request(&event, timeout)
            && let Some(output) = claude::permission_output(&reply, &input)
        {
            println!("{output}");
        }
    } else {
        _ = ipc::send(&event);
    }
    ExitCode::SUCCESS
}

/// What the router remembers of a Claude Code session between prompts.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct RouteState {
    /// Prompts in a row that wanted a lighter model.
    streak: u32,
    /// The prompt being typed again after a switch, which goes through.
    pending: Option<String>,
}

/// For a UserPromptSubmit hook, when the user lets arnotch pick the model of Claude Code
/// in terminals (`terminalModel=auto` in arnotch's settings) and this prompt wants another
/// one: blocks the prompt (the hook's output), and starts switching the session's model and
/// typing the prompt again. `None` lets the prompt through.
fn route_prompt(input: &str) -> Option<String> {
    let hook: serde_json::Value = serde_json::from_str(input).ok()?;
    if hook["hook_event_name"] != "UserPromptSubmit" || setting("terminalModel")? != "auto" {
        return None;
    }
    let prompt = hook["prompt"].as_str()?.trim();
    let session = hook["session_id"].as_str()?;
    let transcript = std::path::Path::new(hook["transcript_path"].as_str().unwrap_or_default());

    let dir = runtime_dir().join("arnotch/route");
    let state_file = dir.join(session.replace('/', "_"));
    let mut state: RouteState = std::fs::read_to_string(&state_file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let save = |state: &RouteState| {
        _ = std::fs::create_dir_all(&dir);
        _ = std::fs::write(
            &state_file,
            serde_json::to_string(state).unwrap_or_default(),
        );
    };
    // The prompt typed again after a switch.
    if state.pending.as_deref() == Some(prompt) {
        state.pending = None;
        save(&state);
        return None;
    }

    // The last answer's model; a new session's is Claude Code's setting.
    let (current, context) = route::transcript_model(transcript)
        .or_else(|| Some((claude_code_model()?, 0)))
        .unwrap_or((Model::Opus, 0));
    let decision = route::for_terminal(prompt, current, context, &mut state.streak);
    let route::Terminal::Switch(target) = decision else {
        save(&state);
        return None;
    };
    // Typing it again needs the terminal Claude Code runs in.
    let Some(terminal) = own_terminal() else {
        save(&state);
        return None;
    };
    let spawned = std::env::current_exe().ok().and_then(|exe| {
        use std::os::unix::process::CommandExt;
        std::process::Command::new(exe)
            .arg("route-resend")
            .arg(serde_json::to_string(&terminal).ok()?)
            .arg(target.model.alias())
            .arg(target.effort.unwrap_or(""))
            .arg(prompt)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Outlives this hook, which Claude Code waits for.
            .process_group(0)
            .spawn()
            .ok()
    });
    if spawned.is_none() {
        save(&state);
        return None;
    }
    state.pending = Some(prompt.to_owned());
    save(&state);
    _ = ipc::send(&Event::Notify {
        title: trf("Claude Code → {}", &[&target.model.name()]),
        body: trf(
            "{} was answering; this prompt suits {}",
            &[&current.name(), &target.model.name()],
        ),
    });
    let reason = trf(
        "arnotch: switching to {}, then sending the prompt again…",
        &[&target.model.name()],
    );
    Some(
        serde_json::json!({
            "decision": "block",
            "reason": reason,
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "suppressOriginalPrompt": true,
            },
        })
        .to_string(),
    )
}

/// Switches the Claude Code session in `terminal` (JSON) to `model` and `effort` (empty
/// to leave it), then types `prompt` again.
fn route_resend(terminal: &str, model: &str, effort: &str, prompt: &str) -> ExitCode {
    let Ok(terminal) = serde_json::from_str::<Terminal>(terminal) else {
        return ExitCode::FAILURE;
    };
    // `/model` and `/effort` also save themselves as the default of new sessions: the
    // user's own are kept aside until they are back (so a switch that didn't finish
    // putting them back doesn't make its models the "defaults" of the next one).
    let kept = runtime_dir().join("arnotch/route/claude-defaults.json");
    _ = std::fs::create_dir_all(runtime_dir().join("arnotch/route"));
    let defaults: serde_json::Value = std::fs::read_to_string(&kept)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .or_else(|| {
            let settings = claude_code_settings()?;
            let defaults = serde_json::json!({
                "model": settings.get("model"),
                "effortLevel": settings.get("effortLevel"),
            });
            _ = std::fs::write(&kept, defaults.to_string());
            Some(defaults)
        })
        .unwrap_or_default();

    // Claude Code shows the block and empties its input first.
    std::thread::sleep(Duration::from_millis(700));
    let mut steps = vec![(format!("/model {model}"), 1200)];
    if !effort.is_empty() {
        steps.push((format!("/effort {effort}"), 1500));
    }
    steps.push((prompt.to_owned(), 1000));
    let mut failure = None;
    for (text, wait) in steps {
        // Slash commands get a moment to open their menu before Enter.
        let typed = control::send_text(&terminal, &text, false).and_then(|()| {
            std::thread::sleep(Duration::from_millis(400));
            control::send_text(&terminal, "", true)
        });
        if let Err(err) = typed {
            failure = Some(err);
            break;
        }
        std::thread::sleep(Duration::from_millis(wait));
        // A conversation with history asks "Switch model?" (the cache is lost): 1 says
        // yes. Without that question the 1 lands in the input, and Backspace takes it out.
        if text.starts_with("/model") {
            let answered = control::send_text(&terminal, "1", false).and_then(|()| {
                std::thread::sleep(Duration::from_millis(600));
                control::send_text(&terminal, "\x7f", false)
            });
            if let Err(err) = answered {
                failure = Some(err);
                break;
            }
            std::thread::sleep(Duration::from_millis(800));
        }
    }

    // Claude Code may still be writing its settings: until they read back as the user's.
    let restored = (0..15).any(|_| {
        if restore_defaults(&defaults) {
            // Written again in the meantime?
            std::thread::sleep(Duration::from_millis(500));
            if restore_defaults(&defaults) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(300));
        false
    });
    if restored {
        _ = std::fs::remove_file(&kept);
    }

    if let Some(err) = failure {
        _ = ipc::send(&Event::Notify {
            title: tr("Can't type into the terminal").into(),
            body: format!("{err:#}"),
        });
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Puts the user's `model` and `effortLevel` back into Claude Code's settings (a missing
/// one is removed). True once they hold them, writing only what differs.
fn restore_defaults(defaults: &serde_json::Value) -> bool {
    let path = claude_code_dir().join("settings.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(mut settings) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let Some(object) = settings.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for key in ["model", "effortLevel"] {
        let wanted = defaults.get(key).filter(|v| !v.is_null());
        if object.get(key) != wanted {
            changed = true;
            match wanted {
                Some(value) => _ = object.insert(key.to_owned(), value.clone()),
                None => _ = object.remove(key),
            }
        }
    }
    if !changed {
        return true;
    }
    let newline = if text.ends_with('\n') { "\n" } else { "" };
    if let Ok(json) = serde_json::to_string_pretty(&settings) {
        _ = std::fs::write(&path, json + newline);
    }
    // Whether it held is seen on the next call.
    false
}

/// The terminal this process runs in: the one whose shell is among its ancestors.
fn own_terminal() -> Option<Terminal> {
    let mut ancestors = Vec::new();
    let mut pid = std::process::id();
    while pid > 1 && ancestors.len() < 32 {
        ancestors.push(pid);
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The fields after the command name, which may hold spaces and parentheses.
        let rest = &stat[stat.rfind(')')? + 2..];
        pid = rest.split(' ').nth(1)?.parse().ok()?;
    }
    let (terminals, _) = terminals::Scanner::new().scan();
    terminals
        .into_iter()
        .find(|t| t.can_send && ancestors.contains(&t.shell_pid))
}

/// Claude Code's model setting (`model` in ~/.claude/settings.json).
fn claude_code_model() -> Option<Model> {
    Model::of_id(claude_code_settings()?["model"].as_str()?)
}

fn claude_code_settings() -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(claude_code_dir().join("settings.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .filter(|v| v.is_object())
}

fn claude_code_dir() -> std::path::PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
}

/// A value from arnotch's settings (~/.config/arnotch/arnotch.conf, which the island
/// writes).
fn setting(key: &str) -> Option<String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    let text = std::fs::read_to_string(config.join("arnotch/arnotch.conf")).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(|value| value.trim().to_owned())
}

fn home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn runtime_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Lets the user select a region of the screen with Spectacle, then asks the island
/// where the picture goes (a Claude session, the chat, the shelf, a share link).
/// Pictures are kept in ~/.cache/arnotch/screenshots, the newest 20.
fn screenshot() -> ExitCode {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        });
    let dir = cache.join("arnotch/screenshots");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("arnotchctl: cannot create {}: {err}", dir.display());
        return ExitCode::FAILURE;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let path = dir.join(format!("screenshot-{stamp}.png"));

    let status = std::process::Command::new("spectacle")
        .args(["--background", "--nonotify", "--region", "--output"])
        .arg(&path)
        .status();
    if let Err(err) = status {
        eprintln!("arnotchctl: cannot run spectacle (KDE's screenshot tool): {err}");
        return ExitCode::FAILURE;
    }
    // No file: the selection was cancelled.
    if !path.is_file() {
        return ExitCode::SUCCESS;
    }

    let mut old: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    old.sort();
    for stale in old.iter().rev().skip(20) {
        _ = std::fs::remove_file(stale);
    }
    send(&Event::Pick {
        paths: vec![path.to_string_lossy().into_owned()],
    })
}

/// Claude Code's status line command: passes the session's cost and the account's rate
/// limits to arnotch, then runs the status line the user had before (saved by
/// install.sh in ~/.config/arnotch/statusline-command) with the same input.
fn statusline() -> ExitCode {
    let mut input = Vec::new();
    if std::io::stdin().read_to_end(&mut input).is_err() {
        return ExitCode::SUCCESS;
    }
    if let Some(usage) = claude::usage_from_statusline(&String::from_utf8_lossy(&input)) {
        _ = ipc::send(&Event::Usage(usage));
    }

    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    let Ok(command) = std::fs::read_to_string(config.join("arnotch/statusline-command")) else {
        return ExitCode::SUCCESS;
    };
    let command = command.trim();
    if command.is_empty() {
        return ExitCode::SUCCESS;
    }
    let child = std::process::Command::new("sh")
        .args(["-c", command])
        .stdin(std::process::Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        return ExitCode::FAILURE;
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        _ = stdin.write_all(&input);
    }
    match child.wait() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// How long a plan approval or a question waits in the island, which takes reading:
/// 5 minutes, or `ARNOTCH_PROMPT_TIMEOUT` seconds. (The hook's own timeout in Claude
/// Code's settings must be longer.)
fn prompt_timeout() -> Duration {
    let secs = std::env::var("ARNOTCH_PROMPT_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    Duration::from_secs(secs)
}

/// How long a permission request waits in the island before falling back to the terminal.
/// `ARNOTCH_PERMISSION_TIMEOUT` (seconds) overrides the default of 45.
fn permission_timeout() -> Duration {
    let secs = std::env::var("ARNOTCH_PERMISSION_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(45);
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_percentages() {
        assert_eq!(
            last_percent("  1,234,567  45%   12.3MB/s    0:01:02"),
            Some(45.0)
        );
        assert_eq!(last_percent("######              32.5%"), Some(32.5));
        assert_eq!(last_percent(" 10% then 20%"), Some(20.0));
        assert_eq!(last_percent("%"), None);
        assert_eq!(last_percent("250%"), None);
        assert_eq!(last_percent("no progress"), None);
    }
}
