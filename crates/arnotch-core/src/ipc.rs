//! Unix socket IPC: clients write newline-delimited JSON [`Event`]s,
//! the notch broadcasts them to every subscriber (the UI, later: plugins).
//!
//! Permission requests are the one exchange that goes both ways: the client keeps
//! its connection open and receives the user's [`Reply`] as one line of JSON.

use std::collections::HashMap;
use std::io::{BufRead, BufReader as StdBufReader, Write};
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, oneshot};

use crate::event::{AgentPhase, Event, Reply};

/// Permission requests waiting for the user, by request id, with when they arrived.
type Pending = HashMap<String, (Instant, oneshot::Sender<Reply>)>;
static PENDING: LazyLock<Mutex<Pending>> = LazyLock::new(Default::default);

/// `$XDG_RUNTIME_DIR/arnotch.sock`, falling back to `/tmp/arnotch-$UID.sock`.
pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("arnotch.sock"),
        None => {
            // SAFETY: getuid has no preconditions and cannot fail.
            let uid = unsafe { libc::getuid() };
            PathBuf::from(format!("/tmp/arnotch-{uid}.sock"))
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

/// Sends a permission request and waits up to `timeout` for the user's reply.
/// `None` means nobody answered (or no notch runs): the caller should fall back.
pub fn request(event: &Event, timeout: Duration) -> std::io::Result<Option<Reply>> {
    let mut stream = StdUnixStream::connect(socket_path())?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    stream.write_all(event.to_line().as_bytes())?;
    stream.set_read_timeout(Some(timeout))?;

    let mut line = String::new();
    match StdBufReader::new(stream).read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(serde_json::from_str(line.trim()).ok()),
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

/// Delivers the user's reply. Returns false when the request is gone
/// (answered elsewhere or abandoned by its client).
pub fn answer(request_id: &str, reply: Reply) -> bool {
    let pending = PENDING.lock().unwrap().remove(request_id);
    pending.is_some_and(|(_, sender)| sender.send(reply).is_ok())
}

/// Answers the newest pending request, e.g. from a keyboard shortcut.
pub fn answer_latest(reply: Reply) -> bool {
    let latest = {
        let pending = PENDING.lock().unwrap();
        pending
            .iter()
            .max_by_key(|(_, (at, _))| *at)
            .map(|(id, _)| id.clone())
    };
    latest.is_some_and(|id| answer(&id, reply))
}

/// Binds the socket, refusing to steal it from a notch that is already running.
pub fn bind() -> anyhow::Result<StdUnixListener> {
    let path = socket_path();
    if path.exists() {
        if StdUnixStream::connect(&path).is_ok() {
            anyhow::bail!("another arnotch is already listening on {}", path.display());
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
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if line.trim().is_empty() => {}
            Ok(Some(line)) => match serde_json::from_str::<Event>(&line) {
                Ok(event) => {
                    if let Some(request_id) = permission_id(&event) {
                        let decision =
                            await_decision(request_id.clone(), event, &events, &mut lines).await;
                        if let Some(decision) = decision {
                            let reply = format!("{}\n", serde_json::to_string(&decision).unwrap());
                            _ = writer.write_all(reply.as_bytes()).await;
                        }
                        _ = events.send(Event::PermissionClosed { request_id });
                        break;
                    }
                    if let Event::Answer { reply } = event {
                        answer_latest(reply);
                        continue;
                    }
                    // Sending only fails when nobody is subscribed; dropping the event is fine then.
                    _ = events.send(event);
                }
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

fn permission_id(event: &Event) -> Option<String> {
    match event {
        Event::Agent {
            phase: AgentPhase::Permission { request_id, .. },
            ..
        } => Some(request_id.clone()),
        _ => None,
    }
}

/// Shows the request and waits for the user, or for the client to give up and disconnect.
async fn await_decision(
    request_id: String,
    event: Event,
    events: &broadcast::Sender<Event>,
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> Option<Reply> {
    let (sender, receiver) = oneshot::channel();
    PENDING
        .lock()
        .unwrap()
        .insert(request_id.clone(), (Instant::now(), sender));
    _ = events.send(event);

    let decision = tokio::select! {
        decision = receiver => decision.ok(),
        // The client never sends more; any read result means it is gone.
        _ = lines.next_line() => None,
    };
    PENDING.lock().unwrap().remove(&request_id);
    decision
}
