//! What's playing, from MPRIS: the D-Bus interface music players and browsers publish
//! (Spotify, Firefox, Chromium, VLC, mpv, ...). The notch listens to their signals, so
//! it does nothing while nothing changes.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::OwnedValue;
use zbus::{MatchRule, Message};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";

/// The player shown in the island.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Player {
    /// D-Bus name, to send commands to.
    pub bus_name: String,
    /// The application, e.g. "Spotify".
    pub identity: String,
    pub title: String,
    pub artist: String,
    /// `file://` or `https://` URL of the cover, empty when there is none.
    pub art_url: String,
    pub playing: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
}

/// A button pressed in the island.
#[derive(Debug, Clone, Copy)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
}

/// Calls `on_change` with the player to show (None when there is none) every time it
/// changes. Runs until the session bus goes away, so call it on its own thread.
pub fn watch(on_change: impl Fn(Option<Player>)) -> zbus::Result<()> {
    let conn = Connection::session()?;
    let (tx, rx) = mpsc::channel();

    // A player changed (track, playback status, ...), or one started or quit.
    let changes = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path(PATH)?
        .build();
    let players = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns("org.mpris.MediaPlayer2")?
        .build();
    for rule in [changes, players] {
        let messages = MessageIterator::for_match_rule(rule, &conn, Some(64))?;
        let tx = tx.clone();
        std::thread::Builder::new()
            .name("notch-media".into())
            .spawn(move || {
                for message in messages {
                    if message.is_err() || tx.send(()).is_err() {
                        break;
                    }
                }
            })
            .expect("can spawn a thread");
    }
    drop(tx);

    let mut chooser = Chooser::default();
    let mut shown = None;
    loop {
        let player = chooser.choose(read_players(&conn));
        if player != shown {
            on_change(player.clone());
            shown = player;
        }
        // Wait for a change, then let a burst of signals (a new track sends several) settle.
        if rx.recv().is_err() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
        while rx.try_recv().is_ok() {}
    }
}

/// Sends a button press to a player.
pub fn command(bus_name: &str, command: Command) -> zbus::Result<()> {
    let method = match command {
        Command::PlayPause => "PlayPause",
        Command::Next => "Next",
        Command::Previous => "Previous",
    };
    let conn = Connection::session()?;
    conn.call_method(
        Some(bus_name),
        PATH,
        Some("org.mpris.MediaPlayer2.Player"),
        method,
        &(),
    )?;
    Ok(())
}

/// Picks the player to show: the one playing (the latest to start, if several are),
/// otherwise the one that played last, so it can be resumed.
#[derive(Default)]
struct Chooser {
    /// When each player last started playing.
    started: HashMap<String, Instant>,
    playing: HashSet<String>,
}

impl Chooser {
    fn choose(&mut self, players: Vec<Player>) -> Option<Player> {
        self.started
            .retain(|name, _| players.iter().any(|p| &p.bus_name == name));
        for player in &players {
            if player.playing && !self.playing.contains(&player.bus_name) {
                self.started.insert(player.bus_name.clone(), Instant::now());
            }
        }
        self.playing = players
            .iter()
            .filter(|p| p.playing)
            .map(|p| p.bus_name.clone())
            .collect();
        let rank = |p: &Player| (p.playing, self.started.get(&p.bus_name).copied());
        players
            .into_iter()
            .filter(|p| !p.title.is_empty())
            .max_by(|a, b| rank(a).cmp(&rank(b)))
    }
}

fn read_players(conn: &Connection) -> Vec<Player> {
    let names: Vec<String> = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListNames",
            &(),
        )
        .and_then(|reply| reply.body().deserialize())
        .unwrap_or_default();
    names
        .into_iter()
        .filter(|name| name.starts_with(PREFIX))
        .filter_map(|name| read_player(conn, name))
        .collect()
}

fn read_player(conn: &Connection, bus_name: String) -> Option<Player> {
    let get_all = |interface: &str| -> Option<HashMap<String, OwnedValue>> {
        let reply: Message = conn
            .call_method(
                Some(bus_name.as_str()),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(interface,),
            )
            .ok()?;
        reply.body().deserialize().ok()
    };
    let player = get_all("org.mpris.MediaPlayer2.Player")?;
    let app = get_all("org.mpris.MediaPlayer2").unwrap_or_default();

    let metadata: HashMap<String, OwnedValue> = player
        .get("Metadata")
        .and_then(|m| m.try_clone().ok())
        .and_then(|m| m.try_into().ok())
        .unwrap_or_default();
    let text = |map: &HashMap<String, OwnedValue>, key: &str| -> String {
        map.get(key)
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| String::try_from(v).ok())
            .unwrap_or_default()
    };
    let flag = |key: &str| -> bool {
        player
            .get(key)
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(false)
    };
    let artist = metadata
        .get("xesam:artist")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .map(|artists| artists.join(", "))
        .unwrap_or_default();

    Some(Player {
        identity: Some(text(&app, "Identity"))
            .filter(|i| !i.is_empty())
            .unwrap_or_else(|| {
                bus_name[PREFIX.len()..]
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_owned()
            }),
        title: text(&metadata, "xesam:title"),
        artist,
        art_url: text(&metadata, "mpris:artUrl"),
        playing: text(&player, "PlaybackStatus") == "Playing",
        can_go_next: flag("CanGoNext"),
        can_go_previous: flag("CanGoPrevious"),
        bus_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(name: &str, playing: bool) -> Player {
        Player {
            bus_name: name.into(),
            identity: name.into(),
            title: "Song".into(),
            artist: String::new(),
            art_url: String::new(),
            playing,
            can_go_next: true,
            can_go_previous: true,
        }
    }

    #[test]
    fn prefers_the_player_that_started_last() {
        let mut chooser = Chooser::default();
        let shown = |p: Option<Player>| p.map(|p| p.bus_name);

        assert!(
            chooser
                .choose(vec![player("a", false), player("b", false)])
                .is_some()
        );
        assert_eq!(
            shown(chooser.choose(vec![player("a", true), player("b", false)])),
            Some("a".into())
        );
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(
            shown(chooser.choose(vec![player("a", true), player("b", true)])),
            Some("b".into())
        );
        // Paused: still shown, as the one that played last.
        assert_eq!(
            shown(chooser.choose(vec![player("a", false), player("b", false)])),
            Some("b".into())
        );
        // Resumed: it started last now, even while the other one plays again.
        assert_eq!(
            shown(chooser.choose(vec![player("a", true), player("b", false)])),
            Some("a".into())
        );
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(
            shown(chooser.choose(vec![player("a", false), player("b", true)])),
            Some("b".into())
        );
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(
            shown(chooser.choose(vec![player("a", true), player("b", true)])),
            Some("a".into())
        );
    }

    #[test]
    fn skips_players_without_a_track() {
        let mut chooser = Chooser::default();
        let mut idle = player("idle", true);
        idle.title.clear();
        assert_eq!(chooser.choose(vec![idle]), None);
    }
}
