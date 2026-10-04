mod app;
mod bridge;

use cxx_qt_lib::{QQmlApplicationEngine, QUrl};
use std::sync::atomic::Ordering;
use std::time::Duration;

use arnotch_core::system::{self, Level};
use arnotch_core::terminals::{Finished, Scanner};
use arnotch_core::{Event, i18n, ipc, media};
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

    spawn_system_watchers()?;

    let (calendar_url, urls) = std::sync::mpsc::channel();
    _ = bridge::CALENDAR_URL.set(calendar_url);
    std::thread::Builder::new()
        .name("notch-calendar".into())
        .spawn(move || watch_calendar(urls))?;

    let (weather_city, cities) = std::sync::mpsc::channel();
    _ = bridge::WEATHER_CITY.set(weather_city);
    std::thread::Builder::new()
        .name("notch-weather".into())
        .spawn(move || watch_weather(cities))?;

    std::thread::Builder::new()
        .name("notch-jobs".into())
        .spawn(|| {
            if let Err(err) = arnotch_core::jobs::watch(bridge::publish_jobs) {
                tracing::info!("no download progress: {err:#}");
            }
        })?;

    std::thread::Builder::new()
        .name("notch-phone".into())
        .spawn(|| {
            let events = bridge::event_bus();
            if let Err(err) =
                arnotch_core::phone::watch(|event| _ = events.send(Event::Phone(event)))
            {
                tracing::info!("no phone (KDE Connect): {err:#}");
            }
        })?;

    std::thread::Builder::new()
        .name("notch-clipboard".into())
        .spawn(|| {
            if let Err(err) = arnotch_core::clipboard::watch_history(bridge::publish_clipboard) {
                tracing::info!("no clipboard history (Klipper): {err:#}");
            }
        })?;

    std::thread::Builder::new()
        .name("notch-media".into())
        .spawn(|| {
            if let Err(err) = media::watch(bridge::publish_media) {
                tracing::warn!("cannot follow media players: {err:#}");
            }
        })?;

    let mut app = app::ffi::new_app();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/dev/arnotch/qml/Main.qml"));
    }
    // Without its QML the notch would run with no window at all.
    if !engine.as_ref().is_some_and(app::ffi::engine_loaded) {
        _ = std::fs::remove_file(ipc::socket_path());
        anyhow::bail!("the interface failed to load, see the QML errors above");
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

/// Volume, brightness and battery, each on its own thread blocked on its events.
fn spawn_system_watchers() -> std::io::Result<()> {
    let events = bridge::event_bus();
    let send = move |event| _ = events.send(event);

    let level = send.clone();
    std::thread::Builder::new()
        .name("notch-volume".into())
        .spawn(move || {
            let result = system::watch_volume(|value, muted| {
                level(Event::Level {
                    kind: Level::Volume,
                    value,
                    muted,
                })
            });
            if let Err(err) = result {
                tracing::info!("no volume indicator: {err:#}");
            }
        })?;
    let level = send.clone();
    std::thread::Builder::new()
        .name("notch-bright".into())
        .spawn(move || {
            let result = system::watch_brightness(|value| {
                level(Event::Level {
                    kind: Level::Brightness,
                    value,
                    muted: false,
                })
            });
            if let Err(err) = result {
                tracing::info!("no brightness indicator: {err:#}");
            }
        })?;
    std::thread::Builder::new()
        .name("notch-battery".into())
        .spawn(move || {
            if let Err(err) = system::watch_battery(|battery| send(Event::Battery { battery })) {
                tracing::info!("no battery indicator: {err:#}");
            }
        })?;
    Ok(())
}

/// Downloads the calendar every 10 minutes, and right away when its address changes.
fn watch_calendar(urls: std::sync::mpsc::Receiver<String>) {
    use arnotch_core::calendar;
    use std::sync::mpsc::RecvTimeoutError;

    let mut url = String::new();
    let mut shown = None;
    loop {
        let wait = if url.is_empty() {
            Duration::MAX
        } else {
            Duration::from_secs(600)
        };
        match urls.recv_timeout(wait) {
            Ok(new) if new == url && shown.is_some() => continue,
            Ok(new) => url = new.trim().to_owned(),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if url.is_empty() {
            shown = None;
            bridge::publish_calendar(Vec::new());
            continue;
        }
        match calendar::fetch(&url) {
            Ok(ics) => {
                let meetings =
                    calendar::upcoming(&ics, chrono::Utc::now(), chrono::Duration::hours(24));
                if shown.as_ref() != Some(&meetings) {
                    shown = Some(meetings.clone());
                    bridge::publish_calendar(meetings);
                }
            }
            // The error never contains the address, which is a secret.
            Err(err) => tracing::warn!("calendar: {err:#}"),
        }
    }
}

/// Downloads the weather every 20 minutes, and right away when the city changes. The city
/// is looked up once; a failed lookup is retried with the next download.
fn watch_weather(cities: std::sync::mpsc::Receiver<String>) {
    use arnotch_core::weather;
    use std::sync::mpsc::RecvTimeoutError;

    let mut city = String::new();
    let mut place = None;
    let mut shown = None;
    loop {
        let wait = if city.is_empty() {
            Duration::MAX
        } else {
            Duration::from_secs(1200)
        };
        match cities.recv_timeout(wait) {
            Ok(new) if new.trim() == city => continue,
            Ok(new) => {
                city = new.trim().to_owned();
                place = None;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if city.is_empty() {
            shown = None;
            bridge::publish_weather(None);
            continue;
        }
        if place.is_none() {
            match weather::find_place(&city, i18n::language()) {
                Ok(found) => place = Some(found),
                Err(err) => {
                    tracing::warn!("weather: {err:#}");
                    continue;
                }
            }
        }
        match weather::current(place.as_ref().expect("found above")) {
            Ok(now) if shown.as_ref() != Some(&now) => {
                shown = Some(now.clone());
                bridge::publish_weather(Some(now));
            }
            Ok(_) => {}
            Err(err) => tracing::warn!("weather: {err:#}"),
        }
    }
}

fn finished_notification(done: &Finished) -> Event {
    let folder = done.cwd.rsplit('/').next().unwrap_or_default();
    Event::Notify {
        title: done.args.clone(),
        body: i18n::trf(
            "Finished after {} · {} · {}",
            &[&i18n::duration(done.duration_secs), &done.app, &folder],
        ),
    }
}
