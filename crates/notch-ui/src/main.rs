mod app;
mod bridge;

use cxx_qt_lib::{QQmlApplicationEngine, QUrl};
use notch_core::ipc;
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
