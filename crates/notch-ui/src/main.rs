mod app;
mod bridge;

use cxx_qt_lib::{QQmlApplicationEngine, QUrl};
use std::sync::atomic::Ordering;
use std::time::Duration;

use notch_core::terminals::{Finished, Scanner};
use notch_core::{Event, ipc};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let listener = ipc::bind()?;
    // The IPC server is tiny: one thread with a single-threaded runtime is plenty.
    std::thread::Builder::new()
        .name("notch-ipc".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("cannot start the IPC runtime");
            if let Err(err) = runtime.block_on(ipc::serve(listener, bridge::event_bus())) {
                tracing::error!("IPC server stopped: {err:#}");
            }
        })?;

    std::thread::Builder::new()
        .name("notch-terminals".into())
        .spawn(watch_terminals)?;

    let mut app = app::ffi::new_app();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/dev/notch/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app::ffi::exec(app);
    }

    _ = std::fs::remove_file(ipc::socket_path());
    Ok(())
}

/// Rescans the open terminals, publishing changes and announcing long commands
/// that finished: every second while the list is on screen, every 5 seconds otherwise.
fn watch_terminals() {
    let events = bridge::event_bus();
    let mut scanner = Scanner::new();
    let mut last = None;
    loop {
        let (terminals, finished) = scanner.scan();
        for done in finished {
            _ = events.send(finished_notification(&done));
        }
        if last.as_ref() != Some(&terminals) {
            last = Some(terminals.clone());
            bridge::publish_terminals(terminals);
        }
        for _ in 0..5 {
            std::thread::sleep(Duration::from_secs(1));
            if bridge::TERMINALS_VISIBLE.load(Ordering::Relaxed) {
                break;
            }
        }
    }
}

fn finished_notification(done: &Finished) -> Event {
    let secs = done.duration_secs;
    let duration = match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, secs % 3600 / 60),
    };
    let folder = done.cwd.rsplit('/').next().unwrap_or_default();
    Event::Notify {
        title: done.args.clone(),
        body: format!("Finished after {duration} · {} · {folder}", done.app),
    }
}
