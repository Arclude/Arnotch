//! Command line client for the notch.
//!
//!   notchctl notify <title> [body]   show a message
//!   notchctl open [tab]              open the island (tabs: activity, terminals)
//!   notchctl close                   close the island
//!   notchctl settings                open the settings window
//!   notchctl terminals               list open terminals and what runs in them
//!   notchctl claude-hook             forward a Claude Code hook payload read from stdin
//!   notchctl send '<json event>'     send a raw event

use std::io::Read;
use std::process::ExitCode;

use notch_core::{Event, claude, ipc};

const USAGE: &str = "usage:
  notchctl notify <title> [body]
  notchctl open [activity|terminals]
  notchctl close
  notchctl settings
  notchctl terminals
  notchctl claude-hook        (reads the hook JSON from stdin)
  notchctl send '<json event>'";

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
        ["close"] => send(&Event::Close),
        ["settings"] => send(&Event::OpenSettings),
        ["terminals"] => {
            let (terminals, _) = notch_core::terminals::Scanner::new().scan();
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
                eprintln!("notchctl: invalid event: {err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn send(event: &Event) -> ExitCode {
    match ipc::send(event) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "notchctl: cannot reach the notch at {}: {err}",
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
    if let Ok(Some(event)) = claude::event_from_hook(&input) {
        _ = ipc::send(&event);
    }
    ExitCode::SUCCESS
}
