//! The phone, through KDE Connect: its battery, incoming and missed calls, and new text
//! messages. Everything comes from KDE Connect's D-Bus signals, so nothing is polled.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::{OwnedValue, Value};

const SERVICE: &str = "org.kde.kdeconnect";
const DEVICES: &str = "/modules/kdeconnect/devices";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhoneEvent {
    /// The phone's battery, sent when it connects and whenever the charge changes.
    Battery {
        device: String,
        percent: i32,
        charging: bool,
    },
    /// The phone went out of reach.
    Gone { device: String },
    /// "ringing", "talking" or "missed_call".
    Call {
        device: String,
        state: String,
        number: String,
        contact: String,
    },
    /// A new text message received.
    Message {
        device: String,
        from: String,
        body: String,
    },
}

/// KDE Connect's name for a call's state as `PhoneEvent::Call` has it. A ringing phone is
/// "callReceived" in current versions, "ringing" in older ones.
fn call_state(event: &str) -> String {
    match event {
        "callReceived" | "ringing" => "ringing",
        "missedCall" => "missed_call",
        other => other,
    }
    .to_owned()
}

/// Calls `on_event` for what the paired phones report. Runs until the session bus goes
/// away; returns an error where KDE Connect isn't installed.
pub fn watch(on_event: impl Fn(PhoneEvent)) -> zbus::Result<()> {
    let conn = Connection::session()?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(SERVICE)?
        .path_namespace(DEVICES)?
        .build();
    // Subscribe first, so nothing is missed between the first read and the signals.
    let messages = MessageIterator::for_match_rule(rule, &conn, Some(64))?;

    let mut names: HashMap<String, String> = HashMap::new();
    let mut name_of = |id: &str| -> String {
        names
            .entry(id.to_owned())
            .or_insert_with(|| {
                device_property::<String>(&conn, id, "name").unwrap_or_else(|| id.to_owned())
            })
            .clone()
    };
    for id in device_ids(&conn) {
        if device_property::<bool>(&conn, id.as_str(), "isReachable") == Some(true)
            && let Some((percent, charging)) = battery(&conn, &id)
        {
            on_event(PhoneEvent::Battery {
                device: name_of(&id),
                percent,
                charging,
            });
        }
    }

    for message in messages {
        let message = message?;
        let header = message.header();
        let (Some(path), Some(member)) = (header.path(), header.member()) else {
            continue;
        };
        // /modules/kdeconnect/devices/<id>[/<plugin>]
        let Some(id) = path
            .as_str()
            .strip_prefix(DEVICES)
            .and_then(|rest| rest.split('/').nth(1))
        else {
            continue;
        };
        let id = id.to_owned();
        let body = message.body();
        match member.as_str() {
            "refreshed" => {
                if let Ok((charging, percent)) = body.deserialize::<(bool, i32)>() {
                    on_event(PhoneEvent::Battery {
                        device: name_of(&id),
                        percent,
                        charging,
                    });
                }
            }
            "reachableChanged" => match battery(&conn, &id) {
                Some((percent, charging))
                    if device_property::<bool>(&conn, &id, "isReachable") == Some(true) =>
                {
                    on_event(PhoneEvent::Battery {
                        device: name_of(&id),
                        percent,
                        charging,
                    })
                }
                _ => on_event(PhoneEvent::Gone {
                    device: name_of(&id),
                }),
            },
            "callReceived" => {
                if let Ok((state, number, contact)) = body.deserialize::<(String, String, String)>()
                {
                    on_event(PhoneEvent::Call {
                        device: name_of(&id),
                        state: call_state(&state),
                        number,
                        contact,
                    });
                }
            }
            "conversationCreated" | "conversationUpdated" => {
                if let Ok(value) = body.deserialize::<OwnedValue>()
                    && let Some(text) = text_message(&value)
                    && text.is_new()
                {
                    on_event(PhoneEvent::Message {
                        device: name_of(&id),
                        from: text.from,
                        body: text.body,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Makes the paired phone called `device` ring loudly, even when it is on silent, to
/// find it (KDE Connect's "Find my phone").
pub fn ring(device: &str) -> anyhow::Result<()> {
    let conn = Connection::session()?;
    let id = device_ids(&conn)
        .into_iter()
        .find(|id| {
            device_property::<String>(&conn, id, "name").as_deref() == Some(device)
                && device_property::<bool>(&conn, id, "isReachable") == Some(true)
        })
        .ok_or_else(|| anyhow::anyhow!(crate::i18n::trf("{} is not in reach", &[&device])))?;
    conn.call_method(
        Some(SERVICE),
        format!("{DEVICES}/{id}/findmyphone").as_str(),
        Some("org.kde.kdeconnect.device.findmyphone"),
        "ring",
        &(),
    )?;
    Ok(())
}

fn device_ids(conn: &Connection) -> Vec<String> {
    conn.call_method(
        Some(SERVICE),
        "/modules/kdeconnect",
        Some("org.kde.kdeconnect.daemon"),
        "devices",
        &(false, true), // only reachable: false, only paired: true
    )
    .and_then(|reply| reply.body().deserialize())
    .unwrap_or_default()
}

fn device_property<T: TryFrom<OwnedValue>>(conn: &Connection, id: &str, name: &str) -> Option<T> {
    property(
        conn,
        &format!("{DEVICES}/{id}"),
        "org.kde.kdeconnect.device",
        name,
    )
}

fn property<T: TryFrom<OwnedValue>>(
    conn: &Connection,
    path: &str,
    interface: &str,
    name: &str,
) -> Option<T> {
    let reply = conn
        .call_method(
            Some(SERVICE),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(interface, name),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    T::try_from(value).ok()
}

fn battery(conn: &Connection, id: &str) -> Option<(i32, bool)> {
    let path = format!("{DEVICES}/{id}/battery");
    let interface = "org.kde.kdeconnect.device.battery";
    let percent = property::<i32>(conn, &path, interface, "charge")?;
    let charging = property::<bool>(conn, &path, interface, "isCharging").unwrap_or(false);
    // KDE Connect reports -1 before the phone sent its battery.
    (percent >= 0).then_some((percent, charging))
}

/// A text message, from KDE Connect's ConversationMessage:
/// (event, body, addresses[(address)], date ms, type, read, threadID, uID, subID, attachments).
struct TextMessage {
    from: String,
    body: String,
    date_ms: i64,
    received: bool,
    unread: bool,
}

impl TextMessage {
    /// Received in the last two minutes and unread: not history synced on connect.
    fn is_new(&self) -> bool {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        self.received && self.unread && now_ms - self.date_ms < 120_000
    }
}

fn text_message(value: &Value) -> Option<TextMessage> {
    let value = match value {
        Value::Value(inner) => inner.as_ref(),
        other => other,
    };
    let Value::Structure(message) = value else {
        return None;
    };
    let fields = message.fields();
    let int = |i: usize| -> Option<i64> {
        match fields.get(i)? {
            Value::I32(n) => Some(*n as i64),
            Value::I64(n) => Some(*n),
            Value::U32(n) => Some(*n as i64),
            _ => None,
        }
    };
    let body = match fields.get(1)? {
        Value::Str(s) => s.to_string(),
        _ => return None,
    };
    let from = match fields.get(2)? {
        Value::Array(addresses) => addresses.iter().find_map(|address| match address {
            Value::Structure(s) => match s.fields().first()? {
                Value::Str(a) => Some(a.to_string()),
                _ => None,
            },
            _ => None,
        }),
        _ => None,
    }
    .unwrap_or_default();
    Some(TextMessage {
        from,
        body,
        date_ms: int(3)?,
        // Android's message types: 1 is inbox (received), 2 is sent.
        received: int(4)? == 1,
        unread: int(5)? == 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_call_states() {
        assert_eq!(call_state("callReceived"), "ringing");
        assert_eq!(call_state("ringing"), "ringing");
        assert_eq!(call_state("missedCall"), "missed_call");
        assert_eq!(call_state("talking"), "talking");
    }
    use zbus::zvariant::{Array, StructureBuilder};

    fn message(body: &str, from: &str, date_ms: i64, kind: i32, read: i32) -> Value<'static> {
        let mut addresses = Array::new(&zbus::zvariant::Signature::try_from("(s)").unwrap());
        addresses
            .append(Value::Structure(
                StructureBuilder::new()
                    .add_field(from.to_owned())
                    .build()
                    .unwrap(),
            ))
            .unwrap();
        Value::Structure(
            StructureBuilder::new()
                .add_field(1i32)
                .add_field(body.to_owned())
                .append_field(Value::Array(addresses))
                .add_field(date_ms)
                .add_field(kind)
                .add_field(read)
                .add_field(7i64)
                .add_field(42i32)
                .add_field(1i64)
                .build()
                .unwrap(),
        )
    }

    #[test]
    fn reads_conversation_messages() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let text = text_message(&message("Selam", "+905321112233", now - 5_000, 1, 0)).unwrap();
        assert_eq!(text.body, "Selam");
        assert_eq!(text.from, "+905321112233");
        assert!(text.is_new());

        // Sent by the user, already read, or synced history: not new.
        assert!(
            !text_message(&message("x", "a", now, 2, 0))
                .unwrap()
                .is_new()
        );
        assert!(
            !text_message(&message("x", "a", now, 1, 1))
                .unwrap()
                .is_new()
        );
        assert!(
            !text_message(&message("x", "a", now - 3_600_000, 1, 0))
                .unwrap()
                .is_new()
        );
        assert!(text_message(&Value::from("not a message")).is_none());
    }
}
