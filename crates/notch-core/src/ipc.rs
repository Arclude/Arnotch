//! Unix socket IPC: clients write newline-delimited JSON [`Event`]s,
//! the notch broadcasts them to every subscriber (the UI, later: plugins).

use std::io::Write;
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

use crate::event::Event;

/// `$XDG_RUNTIME_DIR/notch.sock`, falling back to `/tmp/notch-$UID.sock`.
pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("notch.sock"),
        None => {
            // SAFETY: getuid has no preconditions and cannot fail.
            let uid = unsafe { libc::getuid() };
            PathBuf::from(format!("/tmp/notch-{uid}.sock"))
        }
    }
}

/// Sends one event to a running notch. Uses a short timeout so callers
/// such as Claude Code hooks never hang when the notch is slow.
pub fn send(event: &Event) -> std::io::Result<()> {
    let mut stream = StdUnixStream::connect(socket_path())?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    stream.write_all(event.to_line().as_bytes())
}

/// Binds the socket, refusing to steal it from a notch that is already running.
pub fn bind() -> anyhow::Result<StdUnixListener> {
    let path = socket_path();
    if path.exists() {
        if StdUnixStream::connect(&path).is_ok() {
            anyhow::bail!("another notch is already listening on {}", path.display());
        }
        std::fs::remove_file(&path)?;
    }
    let listener = StdUnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    tracing::info!("listening on {}", path.display());
    Ok(listener)
}

/// Accepts connections forever, forwarding every valid event to `events`.
/// Must be called from within a tokio runtime.
pub async fn serve(
    listener: StdUnixListener,
    events: broadcast::Sender<Event>,
) -> anyhow::Result<()> {
    let listener = UnixListener::from_std(listener)?;
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_client(stream, events.clone()));
    }
}

async fn handle_client(stream: UnixStream, events: broadcast::Sender<Event>) {
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if line.trim().is_empty() => {}
            Ok(Some(line)) => match serde_json::from_str::<Event>(&line) {
                // Sending only fails when nobody is subscribed; dropping the event is fine then.
                Ok(event) => _ = events.send(event),
                Err(err) => tracing::warn!("ignoring invalid event {line:?}: {err}"),
            },
            Ok(None) => break,
            Err(err) => {
                tracing::warn!("client read failed: {err}");
                break;
            }
        }
    }
}
