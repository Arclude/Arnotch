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

/// A permission request waiting for the user.
struct Waiting {
    arrived: Instant,
    session_id: String,
    agent_id: String,
    /// The tool asking, and its call's id when Claude Code sends one (empty otherwise).
    tool: String,
    tool_use_id: String,
    sender: oneshot::Sender<Reply>,
}

/// Permission requests waiting for the user, by request id.
type Pending = HashMap<String, Waiting>;
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
    pending.is_some_and(|waiting| waiting.sender.send(reply).is_ok())
}

/// Answers the newest pending request, e.g. from a keyboard shortcut.
pub fn answer_latest(reply: Reply) -> bool {
    let latest = {
        let pending = PENDING.lock().unwrap();
        pending
            .iter()
            .max_by_key(|(_, waiting)| waiting.arrived)
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
                    settle(&event);
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

/// Claude Code shows its own prompt beside the island's, and doesn't tell the hook
/// when the user answers there: the session carrying on means the answer was given.
/// Settled requests are dropped, which closes them in the island and lets their hook
/// exit without an answer.
fn settle(event: &Event) {
    let Event::Agent {
        session_id,
        tool_use_id,
        agent_id,
        phase,
        ..
    } = event
    else {
        return;
    };
    let new_request = permission_id(event);
    PENDING.lock().unwrap().retain(|request_id, waiting| {
        if waiting.session_id != *session_id {
            return true;
        }
        let answered = match phase {
            // A new prompt, the end of the turn or of the session.
            AgentPhase::Started { .. } | AgentPhase::Done | AgentPhase::Ended => true,
            // The tool ran (or failed): it was allowed. Without the call's id, the same
            // agent's next result of that tool tells.
            AgentPhase::ToolResult { tool, .. } => {
                if waiting.tool_use_id.is_empty() {
                    waiting.agent_id == *agent_id && waiting.tool == *tool
                } else {
                    waiting.tool_use_id == *tool_use_id
                }
            }
            // Claude Code shows one prompt at a time.
            AgentPhase::Permission { .. } => new_request.as_ref() != Some(request_id),
            // The same agent moved on to its next tool. Tools started together come
            // within moments of each other, so only a later one counts.
            AgentPhase::Tool { .. } => {
                waiting.agent_id == *agent_id
                    && *tool_use_id != waiting.tool_use_id
                    && waiting.arrived.elapsed() > Duration::from_secs(2)
            }
            AgentPhase::Waiting { .. } => false,
        };
        if answered {
            tracing::info!("permission request {request_id} was answered in the terminal");
        }
        !answered
    });
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
    let (session_id, agent_id, tool, tool_use_id) = match &event {
        Event::Agent {
            session_id,
            agent_id,
            tool_use_id,
            phase: AgentPhase::Permission { tool, .. },
            ..
        } => (
            session_id.clone(),
            agent_id.clone(),
            tool.clone(),
            tool_use_id.clone(),
        ),
        _ => Default::default(),
    };
    PENDING.lock().unwrap().insert(
        request_id.clone(),
        Waiting {
            arrived: Instant::now(),
            session_id,
            agent_id,
            tool,
            tool_use_id,
            sender,
        },
    );
    _ = events.send(event);

    let decision = tokio::select! {
        decision = receiver => decision.ok(),
        // The client never sends more; any read result means it is gone.
        _ = lines.next_line() => None,
    };
    PENDING.lock().unwrap().remove(&request_id);
    decision
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(session: &str, tool_use_id: &str, phase: AgentPhase) -> Event {
        Event::Agent {
            session_id: session.into(),
            project: "p".into(),
            cwd: String::new(),
            tool_use_id: tool_use_id.into(),
            agent_id: String::new(),
            agent: String::new(),
            phase,
        }
    }

    fn wait(request_id: &str, session: &str, age: Duration) -> oneshot::Receiver<Reply> {
        wait_for(request_id, session, age, request_id)
    }

    /// A request for Bash; `tool_use_id` may be empty, as some Claude Code versions send none.
    fn wait_for(
        request_id: &str,
        session: &str,
        age: Duration,
        tool_use_id: &str,
    ) -> oneshot::Receiver<Reply> {
        let (sender, receiver) = oneshot::channel();
        PENDING.lock().unwrap().insert(
            request_id.into(),
            Waiting {
                arrived: Instant::now() - age,
                session_id: session.into(),
                agent_id: String::new(),
                tool: "Bash".into(),
                tool_use_id: tool_use_id.into(),
                sender,
            },
        );
        receiver
    }

    fn pending(request_id: &str) -> bool {
        PENDING.lock().unwrap().contains_key(request_id)
    }

    #[test]
    fn requests_answered_in_the_terminal_are_settled() {
        let tool = || AgentPhase::Tool {
            tool: "Read".into(),
            detail: String::new(),
            diff: None,
        };
        let result = || AgentPhase::ToolResult {
            tool: "Bash".into(),
            ok: true,
            output: Vec::new(),
        };

        let _a = wait("t-a", "s1", Duration::ZERO);
        let _other = wait("t-other", "s2", Duration::ZERO);
        // Another tool's result, or a tool started alongside: still waiting.
        settle(&agent("s1", "t-b", result()));
        settle(&agent("s1", "t-b", tool()));
        assert!(pending("t-a"));
        // Its own result: allowed in the terminal.
        settle(&agent("s1", "t-a", result()));
        assert!(!pending("t-a"));

        // A later tool of the same agent means the request was answered.
        let _c = wait("t-c", "s1", Duration::from_secs(5));
        settle(&agent("s1", "t-d", tool()));
        assert!(!pending("t-c"));

        let mut e = wait("t-e", "s1", Duration::ZERO);
        settle(&agent("s1", "", AgentPhase::Done));
        assert!(!pending("t-e"));
        // The hook's wait ends without an answer.
        assert!(e.try_recv().is_err());

        // Without a tool call id, the tool's next result settles it, not another tool's.
        let _f = wait_for("s1-123", "s1", Duration::ZERO, "");
        let read_result = AgentPhase::ToolResult {
            tool: "Read".into(),
            ok: true,
            output: Vec::new(),
        };
        settle(&agent("s1", "t-g", read_result));
        assert!(pending("s1-123"));
        settle(&agent("s1", "t-h", result()));
        assert!(!pending("s1-123"));

        // Other sessions are left alone.
        assert!(pending("t-other"));
        settle(&agent(
            "s2",
            "",
            AgentPhase::Started {
                prompt: String::new(),
            },
        ));
        assert!(!pending("t-other"));
    }
}
