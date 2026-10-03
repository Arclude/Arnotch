//! Command line client for the notch.
//!
//!   arnotchctl notify <title> [body]   show a message
//!   arnotchctl open [tab]              open the island (tabs: activity, chat, terminals, shelf)
//!   arnotchctl toggle [tab]            open the island on a tab, or close it if it shows that tab
//!   arnotchctl close                   close the island
//!   arnotchctl shelf <file>...         put files on the shelf
//!   arnotchctl share <file>            upload a file and copy its link
//!   arnotchctl answer <decision>       answer Claude's latest permission request
//!                                    (allow, deny, ask)
//!   arnotchctl settings                open the settings window
//!   arnotchctl terminals               list open terminals and what runs in them
//!   arnotchctl claude-hook             forward a Claude Code hook payload read from stdin
//!   arnotchctl send '<json event>'     send a raw event

use std::io::Read;
use std::process::ExitCode;
use std::time::Duration;

use arnotch_core::{AgentPhase, Decision, Event, claude, ipc};

const USAGE: &str = "usage:
  arnotchctl notify <title> [body]
  arnotchctl open [activity|chat|terminals|shelf]
  arnotchctl toggle [activity|chat|terminals|shelf]
  arnotchctl close
  arnotchctl shelf <file>...
  arnotchctl share <file>
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
        ["terminals"] => {
            let (terminals, _) = arnotch_core::terminals::Scanner::new().scan();
            println!(
                "{}",
                serde_json::to_string_pretty(&terminals).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        ["claude-hook"] => claude_hook(),
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
