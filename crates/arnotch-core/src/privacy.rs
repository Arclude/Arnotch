//! Which apps use the microphone, the camera or record the screen, for the island's
//! privacy dots (orange, green and purple, as on a phone).
//!
//! PipeWire knows every audio and video stream: `pw-dump --monitor` prints its objects
//! and then each change, so nothing is polled. Apps that open a camera themselves
//! (`/dev/video*`, as most browsers and video call apps do) bypass PipeWire; inotify
//! tells when such a device is opened or closed, and `/proc` then tells by whom.

use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::io::BufReader;
use std::os::unix::ffi::OsStrExt;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Apps by name, each list sorted and without repeats.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InUse {
    pub microphone: Vec<String>,
    pub camera: Vec<String>,
    pub screen: Vec<String>,
}

impl InUse {
    pub const NONE: InUse = InUse {
        microphone: Vec::new(),
        camera: Vec::new(),
        screen: Vec::new(),
    };

    /// Both together, each app once per list.
    pub fn merge(&self, other: &InUse) -> InUse {
        let join = |a: &[String], b: &[String]| -> Vec<String> {
            a.iter()
                .chain(b)
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        InUse {
            microphone: join(&self.microphone, &other.microphone),
            camera: join(&self.camera, &other.camera),
            screen: join(&self.screen, &other.screen),
        }
    }
}

/// Calls `on_change` with the PipeWire streams that capture the microphone, a camera or
/// the screen, whenever that changes. Runs until `pw-dump` exits, so call it on its own
/// thread.
pub fn watch_pipewire(on_change: impl Fn(InUse)) -> anyhow::Result<()> {
    let mut child = Command::new("pw-dump")
        .args(["--monitor", "--no-colors"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));

    let mut graph = Graph::default();
    let mut shown = InUse::default();
    for batch in serde_json::Deserializer::from_reader(stdout).into_iter::<Vec<Value>>() {
        for object in batch? {
            graph.apply(object);
        }
        let now = graph.in_use();
        if now != shown {
            on_change(now.clone());
            shown = now;
        }
    }
    _ = child.wait();
    Ok(())
}

/// PipeWire's objects by id, as `pw-dump` describes them.
#[derive(Default)]
struct Graph {
    objects: HashMap<u64, Value>,
}

impl Graph {
    /// A new object, a change to one (only what changed may be given), or a removal
    /// (`"info": null`).
    fn apply(&mut self, object: Value) {
        let Some(id) = object.get("id").and_then(Value::as_u64) else {
            return;
        };
        if object.get("info").is_none_or(Value::is_null) {
            self.objects.remove(&id);
            return;
        }
        match self.objects.get_mut(&id) {
            Some(Value::Object(known)) => {
                let Value::Object(changes) = object else {
                    return;
                };
                for (key, value) in changes {
                    match (key.as_str(), known.get_mut("info"), value) {
                        ("info", Some(Value::Object(info)), Value::Object(new)) => {
                            info.extend(new);
                        }
                        (_, _, value) => {
                            known.insert(key, value);
                        }
                    }
                }
            }
            _ => {
                self.objects.insert(id, object);
            }
        }
    }

    fn props(&self, id: u64) -> Option<&serde_json::Map<String, Value>> {
        self.objects
            .get(&id)?
            .get("info")?
            .get("props")?
            .as_object()
    }

    /// The nodes linked into `id`'s inputs.
    fn sources_of(&self, id: u64) -> impl Iterator<Item = u64> + '_ {
        self.objects.values().filter_map(move |object| {
            let info = object.get("info")?;
            (object.get("type")?.as_str()? == "PipeWire:Interface:Link"
                && info.get("input-node-id")?.as_u64()? == id)
                .then(|| info.get("output-node-id")?.as_u64())?
        })
    }

    fn in_use(&self) -> InUse {
        let mut microphone = BTreeSet::new();
        let mut camera = BTreeSet::new();
        let mut screen = BTreeSet::new();
        for (&id, object) in &self.objects {
            let Some(info) = object.get("info") else {
                continue;
            };
            if object.get("type").and_then(Value::as_str) != Some("PipeWire:Interface:Node")
                || info.get("state").and_then(Value::as_str) != Some("running")
            {
                continue;
            }
            let Some(props) = info.get("props").and_then(Value::as_object) else {
                continue;
            };
            let text = |key: &str| props.get(key).and_then(Value::as_str).unwrap_or_default();
            // Filters (echo cancelling, ...) carry other streams; the apps behind them count.
            if props.get("node.virtual").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let Some(app) = [
                "application.name",
                "application.process.binary",
                "node.name",
            ]
            .into_iter()
            .map(text)
            .find(|name| !name.is_empty()) else {
                continue;
            };
            // Volume meters of the desktop's sound settings only peek at the level.
            if text("media.name") == "Peak detect"
                || matches!(
                    text("application.process.binary"),
                    "plasmashell" | "pavucontrol" | "pavucontrol-qt"
                )
            {
                continue;
            }
            let sources: Vec<_> = self
                .sources_of(id)
                .filter_map(|source| self.props(source))
                .collect();
            match text("media.class") {
                "Stream/Input/Audio" => {
                    // Recording what plays (a sink's monitor) is not listening.
                    let hears = sources.is_empty()
                        || sources.iter().any(|source| {
                            source
                                .get("media.class")
                                .and_then(Value::as_str)
                                .is_some_and(|class| class.starts_with("Audio/Source"))
                        });
                    if hears {
                        microphone.insert(app.to_owned());
                    }
                }
                "Stream/Input/Video" => {
                    // Cameras are devices; a screen cast comes from the compositor.
                    let from_camera = sources
                        .iter()
                        .any(|source| source.contains_key("device.api"));
                    if from_camera {
                        camera.insert(app.to_owned());
                    } else {
                        screen.insert(app.to_owned());
                    }
                }
                _ => {}
            }
        }
        InUse {
            microphone: microphone.into_iter().collect(),
            camera: camera.into_iter().collect(),
            screen: screen.into_iter().collect(),
        }
    }
}

/// Calls `on_change` with the apps that have a camera (`/dev/video*`) open, whenever that
/// changes. Runs as long as `/dev` exists, so call it on its own thread.
pub fn watch_cameras(on_change: impl Fn(Vec<String>)) -> anyhow::Result<()> {
    let inotify = Inotify::new()?;
    inotify.watch("/dev", libc::IN_CREATE | libc::IN_DELETE)?;
    let watch_devices = || {
        for device in camera_devices() {
            // Already watched devices are only updated.
            _ = inotify.watch(&device, libc::IN_OPEN | libc::IN_CLOSE);
        }
    };
    watch_devices();

    let mut shown = camera_users();
    if !shown.is_empty() {
        on_change(shown.clone());
    }
    loop {
        inotify.wait(None)?;
        // Browsers open every camera for a moment to list them: wait until things settle.
        while inotify.wait(Some(Duration::from_millis(400)))? {}
        watch_devices();
        let now = camera_users();
        if now != shown {
            on_change(now.clone());
            shown = now;
        }
    }
}

fn camera_devices() -> Vec<std::path::PathBuf> {
    std::fs::read_dir("/dev")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().as_bytes().starts_with(b"video"))
        .map(|entry| entry.path())
        .collect()
}

/// The names of the processes holding a camera open, except PipeWire's own (the streams
/// it serves are reported by [`watch_pipewire`]).
fn camera_users() -> Vec<String> {
    let me = std::process::id().to_string();
    let mut users = BTreeSet::new();
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let pid = entry.file_name();
        let Some(pid) = pid
            .to_str()
            .filter(|p| p.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue; // another user's process
        };
        let holds = fds.flatten().any(|fd| {
            std::fs::read_link(fd.path())
                .is_ok_and(|target| target.as_os_str().as_bytes().starts_with(b"/dev/video"))
        });
        if !holds {
            continue;
        }
        let name = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
        let name = name.trim();
        if !name.is_empty() && !matches!(name, "pipewire" | "wireplumber") {
            users.insert(name.to_owned());
        }
    }
    users.into_iter().collect()
}

struct Inotify(libc::c_int);

impl Inotify {
    fn new() -> std::io::Result<Self> {
        // SAFETY: plain system call; the descriptor is owned by the returned value.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Inotify(fd))
    }

    fn watch(&self, path: impl AsRef<std::path::Path>, mask: u32) -> std::io::Result<()> {
        let path = CString::new(path.as_ref().as_os_str().as_bytes())?;
        // SAFETY: `path` is a valid C string for the duration of the call.
        if unsafe { libc::inotify_add_watch(self.0, path.as_ptr(), mask) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Waits for events (at most `timeout`) and reads them; false when none came.
    fn wait(&self, timeout: Option<Duration>) -> std::io::Result<bool> {
        let mut poll = libc::pollfd {
            fd: self.0,
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = timeout.map_or(-1, |t| t.as_millis() as libc::c_int);
        // SAFETY: `poll` points to one valid pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, ms) };
        if ready < 0 {
            let err = std::io::Error::last_os_error();
            return if err.kind() == std::io::ErrorKind::Interrupted {
                Ok(false)
            } else {
                Err(err)
            };
        }
        if ready == 0 {
            return Ok(false);
        }
        // What happened doesn't matter, only that something did.
        let mut buffer = [0u8; 4096];
        // SAFETY: the buffer is valid for its length.
        let read = unsafe { libc::read(self.0, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(true)
    }
}

impl Drop for Inotify {
    fn drop(&mut self) {
        // SAFETY: the descriptor is owned and closed once.
        unsafe { libc::close(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: u64, class: &str, state: &str, props: Value) -> Value {
        let mut props = props;
        props["media.class"] = json!(class);
        json!({ "id": id, "type": "PipeWire:Interface:Node", "info": { "state": state, "props": props } })
    }

    fn link(id: u64, from: u64, to: u64) -> Value {
        json!({ "id": id, "type": "PipeWire:Interface:Link",
                "info": { "output-node-id": from, "input-node-id": to, "props": {} } })
    }

    fn graph(objects: Vec<Value>) -> Graph {
        let mut graph = Graph::default();
        for object in objects {
            graph.apply(object);
        }
        graph
    }

    #[test]
    fn finds_apps_listening() {
        let mut graph = graph(vec![
            node(1, "Audio/Source", "running", json!({ "node.name": "mic" })),
            node(
                2,
                "Audio/Sink",
                "running",
                json!({ "node.name": "speakers" }),
            ),
            node(
                10,
                "Stream/Input/Audio",
                "running",
                json!({ "application.name": "Brave" }),
            ),
            link(20, 1, 10),
            // Records what plays: not the microphone.
            node(
                11,
                "Stream/Input/Audio",
                "running",
                json!({ "application.name": "OBS" }),
            ),
            link(21, 2, 11),
            // A filter in between
            node(
                12,
                "Stream/Input/Audio",
                "running",
                json!({ "node.name": "echo-cancel-capture", "node.virtual": true }),
            ),
            node(
                13,
                "Stream/Input/Audio",
                "running",
                json!({ "application.name": "plasmashell", "application.process.binary": "plasmashell" }),
            ),
            // Idle streams don't count.
            node(
                14,
                "Stream/Input/Audio",
                "idle",
                json!({ "application.name": "Discord" }),
            ),
        ]);
        assert_eq!(graph.in_use().microphone, ["Brave"]);

        // Paused: only the state is sent.
        graph.apply(json!({ "id": 10, "info": { "state": "suspended" } }));
        assert!(graph.in_use().microphone.is_empty());
        graph.apply(json!({ "id": 10, "info": { "state": "running" } }));
        assert_eq!(graph.in_use().microphone, ["Brave"]);
        // Gone
        graph.apply(json!({ "id": 10, "info": null }));
        assert!(graph.in_use().microphone.is_empty());
    }

    #[test]
    fn tells_cameras_from_screen_casts() {
        let graph = graph(vec![
            node(
                1,
                "Video/Source",
                "running",
                json!({ "device.api": "v4l2" }),
            ),
            node(
                2,
                "Video/Source",
                "running",
                json!({ "node.name": "kwin_wayland" }),
            ),
            node(
                10,
                "Stream/Input/Video",
                "running",
                json!({ "application.name": "Firefox" }),
            ),
            link(20, 1, 10),
            node(
                11,
                "Stream/Input/Video",
                "running",
                json!({ "application.name": "OBS" }),
            ),
            link(21, 2, 11),
        ]);
        let in_use = graph.in_use();
        assert_eq!(in_use.camera, ["Firefox"]);
        assert_eq!(in_use.screen, ["OBS"]);
    }

    #[test]
    fn merges() {
        let a = InUse {
            camera: vec!["zoom".into()],
            ..Default::default()
        };
        let b = InUse {
            camera: vec!["Brave".into(), "zoom".into()],
            microphone: vec!["Brave".into()],
            ..Default::default()
        };
        let both = a.merge(&b);
        assert_eq!(both.camera, ["Brave", "zoom"]);
        assert_eq!(both.microphone, ["Brave"]);
    }
}
