//! The Gmail inbox over IMAP, with curl and a Google app password kept in the Secret
//! Service (KWallet, GNOME Keyring), never in the settings. Only reads: curl's own fetch
//! marks a message read, so messages that were unread are marked unread again.

use std::collections::HashSet;
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};

const SERVER: &str = "imaps://imap.gmail.com/INBOX";
/// The inbox lists this many of the newest messages.
pub const SHOWN: usize = 15;
/// How much of a new message is downloaded, for its headers and the start of its text.
const PEEK_BYTES: usize = 16_000;
/// The snippet is cut at this many characters.
const SNIPPET_CHARS: usize = 180;
/// Separates the messages downloaded by one curl run (written by `--write-out`).
const END_MARK: &str = "\u{1}arnotch-end\u{1}";

/// A message in the inbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mail {
    pub uid: u32,
    /// Gmail's message id (hex), as the Gmail API and Claude's Gmail tools know it.
    pub id: String,
    /// The sender's name, or the address when there is none.
    pub from: String,
    pub address: String,
    pub subject: String,
    /// The start of the text, on one line.
    pub snippet: String,
    /// Unix seconds, when Gmail received it.
    pub at: i64,
    pub unread: bool,
    /// A delivery failure notice (mailer-daemon): a message the user sent didn't arrive.
    pub bounce: bool,
    /// Opens the message in Gmail in the browser.
    pub link: String,
}

/// The account and its app password.
pub struct Login {
    pub address: String,
    password: String,
}

impl Login {
    /// The app password saved for `address`.
    pub fn load(address: &str) -> anyhow::Result<Self> {
        let out = Command::new("secret-tool")
            .args(["lookup", "service", "arnotch-mail", "account", address])
            .output()
            .context("cannot run secret-tool (install libsecret)")?;
        let password = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if password.is_empty() {
            bail!(crate::i18n::trf(
                "No app password saved for {}: add one in the settings",
                &[&address]
            ));
        }
        Ok(Self {
            address: address.to_owned(),
            password,
        })
    }
}

/// Saves the app password for `address` in the Secret Service; an empty one removes it.
pub fn save_password(address: &str, password: &str) -> anyhow::Result<()> {
    let attrs = ["service", "arnotch-mail", "account", address];
    // Google shows app passwords in groups of four.
    let password: String = password.chars().filter(|c| !c.is_whitespace()).collect();
    if password.is_empty() {
        Command::new("secret-tool")
            .arg("clear")
            .args(attrs)
            .status()
            .context("cannot run secret-tool")?;
        return Ok(());
    }
    let mut child = Command::new("secret-tool")
        .args(["store", "--label", &format!("arnotch: Gmail {address}")])
        .args(attrs)
        .stdin(Stdio::piped())
        .spawn()
        .context("cannot run secret-tool (install libsecret)")?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(password.as_bytes())?;
    if !child.wait()?.success() {
        bail!("secret-tool could not save the password");
    }
    Ok(())
}

/// Whether an app password is saved for `address`.
pub fn has_password(address: &str) -> bool {
    Login::load(address).is_ok()
}

/// One step of a curl run: an IMAP command on the inbox, or a download of `url`.
struct Transfer {
    url: String,
    request: Option<String>,
}

impl Transfer {
    fn command(request: String) -> Self {
        Self {
            url: SERVER.to_owned(),
            request: Some(request),
        }
    }

    /// The start of a message: its headers and the beginning of its text. curl fetches it
    /// without PEEK, which marks it read.
    fn peek(uid: u32) -> Self {
        Self {
            url: format!("{SERVER}/;UID={uid}/;PARTIAL=0.{PEEK_BYTES}"),
            request: None,
        }
    }
}

/// Runs the transfers one after the other in one curl run, over one login, and returns
/// what each wrote. Each command fetches one message at most: curl writes nothing for a
/// FETCH of several. The login goes to curl on stdin with the rest of its config, so it
/// never shows in `ps`.
fn curl(login: &Login, transfers: &[Transfer]) -> anyhow::Result<Vec<String>> {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let user = quote(&format!("{}:{}", login.address, login.password));
    let config = transfers
        .iter()
        .map(|t| {
            let mut segment = format!(
                "silent\nshow-error\nmax-time = 60\nuser = {user}\nurl = {}\nwrite-out = {}\n",
                quote(&t.url),
                quote(END_MARK)
            );
            if let Some(request) = &t.request {
                segment.push_str(&format!("request = {}\n", quote(request)));
            }
            segment
        })
        .collect::<Vec<_>>()
        .join("next\n");
    let mut child = Command::new("curl")
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot run curl")?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(config.as_bytes())?;
    let out = child.wait_with_output()?;
    if out.status.code() == Some(67) {
        bail!(crate::i18n::tr(
            "Gmail refused the login: check the app password"
        ));
    }
    let parts = split_transfers(&String::from_utf8_lossy(&out.stdout));
    // One step failing (a message deleted meanwhile) leaves its part empty; nothing
    // written means curl didn't get through.
    if parts.len() < transfers.len() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        bail!(crate::i18n::trf("IMAP failed: {}", &[&err]));
    }
    Ok(parts)
}

/// What the inbox looks like between checks.
#[derive(Default)]
pub struct Inbox {
    /// The newest UID seen; 0 before the first check.
    last_uid: u32,
    mails: Vec<Mail>,
}

impl Inbox {
    /// The messages shown after the last check.
    pub fn mails(&self) -> Vec<Mail> {
        self.mails.clone()
    }

    /// Looks at the inbox: returns the messages to show, newest first, and those that
    /// arrived since the last check (none on the first one).
    pub fn check(&mut self, login: &Login) -> anyhow::Result<(Vec<Mail>, Vec<Mail>)> {
        let first = self.last_uid == 0;
        // The messages shown and any newer; on the first check, the last 30 days.
        let range = if first {
            let since = chrono::Local::now() - chrono::Duration::days(30);
            format!("SINCE {}", since.format("%-d-%b-%Y"))
        } else {
            let oldest = self.mails.iter().map(|m| m.uid).min();
            format!("UID {}:*", oldest.unwrap_or(self.last_uid + 1))
        };
        let found = curl(
            login,
            &[
                Transfer::command(format!("UID SEARCH {range}")),
                Transfer::command(format!("UID SEARCH UNSEEN {range}")),
            ],
        )?;
        let present: HashSet<u32> = parse_search(&found[0]).into_iter().collect();
        let unseen: HashSet<u32> = parse_search(&found[1]).into_iter().collect();
        let mut new: Vec<u32> = present
            .iter()
            .copied()
            .filter(|&uid| uid > self.last_uid)
            .collect();
        new.sort_unstable();
        let newest = new.last().copied();
        let new: Vec<u32> = new.iter().rev().take(SHOWN).rev().copied().collect();

        let mut arrived = Vec::new();
        if !new.is_empty() {
            let mut steps: Vec<Transfer> = new
                .iter()
                .flat_map(|&uid| {
                    [
                        Transfer::command(format!(
                            "UID FETCH {uid} (UID FLAGS INTERNALDATE X-GM-MSGID X-GM-THRID)"
                        )),
                        Transfer::peek(uid),
                    ]
                })
                .collect();
            // Reading them marked them read: those that weren't are marked unread again.
            let unread: Vec<u32> = new.iter().copied().filter(|u| unseen.contains(u)).collect();
            if !unread.is_empty() {
                steps.push(Transfer::command(format!(
                    "UID STORE {} -FLAGS (\\Seen)",
                    uid_set(&unread)
                )));
            }
            let parts = curl(login, &steps)?;
            for pair in parts.chunks(2).take(new.len()) {
                let Some(meta) = parse_fetch(&pair[0]).into_iter().next() else {
                    continue;
                };
                let mut mail = mail_from(&meta, &parse_message(&pair[1]), &login.address);
                mail.unread = unseen.contains(&mail.uid);
                arrived.push(mail);
            }
        }

        let mut mails: Vec<Mail> = std::mem::take(&mut self.mails)
            .into_iter()
            .filter(|mail| present.contains(&mail.uid))
            .map(|mut mail| {
                mail.unread = unseen.contains(&mail.uid);
                mail
            })
            .collect();
        mails.extend(arrived.iter().cloned());
        mails.sort_by_key(|m| std::cmp::Reverse(m.uid));
        mails.truncate(SHOWN);
        self.mails = mails.clone();
        if let Some(newest) = newest {
            self.last_uid = self.last_uid.max(newest);
        } else if first {
            // An empty inbox: look for anything from now on.
            self.last_uid = 1;
        }
        Ok((mails, if first { Vec::new() } else { arrived }))
    }
}

fn split_transfers(out: &str) -> Vec<String> {
    let mut parts: Vec<String> = out.split(END_MARK).map(str::to_owned).collect();
    parts.pop(); // after the last mark
    parts
}

fn mail_from(meta: &Meta, message: &Message, account: &str) -> Mail {
    Mail {
        uid: meta.uid,
        id: format!("{:x}", meta.msgid),
        from: if message.name.is_empty() {
            message.address.clone()
        } else {
            message.name.clone()
        },
        address: message.address.clone(),
        subject: message.subject.clone(),
        snippet: message.snippet.clone(),
        at: meta.at,
        unread: !meta.seen,
        bounce: is_bounce(&message.address, &message.subject, &message.content_type),
        link: gmail_link(account, meta.thrid, meta.msgid),
    }
}

/// Opens the message in Gmail, signed in as `account`.
pub fn gmail_link(account: &str, thread: u64, message: u64) -> String {
    format!(
        "https://mail.google.com/mail/?authuser={account}#all/thread-f:{thread}%7Cmsg-f:{message}"
    )
}

/// A delivery failure notice.
pub fn is_bounce(address: &str, subject: &str, content_type: &str) -> bool {
    let address = address.to_ascii_lowercase();
    let local = address.split('@').next().unwrap_or_default();
    let subject = subject.to_lowercase();
    local == "mailer-daemon"
        || local == "postmaster"
        || content_type
            .to_ascii_lowercase()
            .contains("report-type=delivery-status")
        || subject.starts_with("delivery status notification")
        || subject.starts_with("undeliverable:")
        || subject.starts_with("undelivered mail returned")
}

/// `* SEARCH 4 7 12` → [4, 7, 12]
fn parse_search(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("* SEARCH"))
        .flat_map(|rest| rest.split_whitespace().filter_map(|n| n.parse().ok()))
        .collect()
}

/// 1,2,3,7 → "1:3,7"
fn uid_set(uids: &[u32]) -> String {
    let mut uids = uids.to_vec();
    uids.sort_unstable();
    uids.dedup();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for uid in uids {
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == uid => *end = uid,
            _ => ranges.push((uid, uid)),
        }
    }
    ranges
        .iter()
        .map(|&(a, b)| {
            if a == b {
                a.to_string()
            } else {
                format!("{a}:{b}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug, PartialEq)]
struct Meta {
    uid: u32,
    seen: bool,
    msgid: u64,
    thrid: u64,
    at: i64,
}

/// `* 5 FETCH (X-GM-THRID 1 X-GM-MSGID 2 UID 12 FLAGS (\Seen) INTERNALDATE "...")` lines.
fn parse_fetch(text: &str) -> Vec<Meta> {
    text.lines().filter_map(parse_fetch_line).collect()
}

fn parse_fetch_line(line: &str) -> Option<Meta> {
    let rest = line.trim().strip_prefix("* ")?;
    let (_, rest) = rest.split_once(" FETCH (")?;
    let rest = rest.strip_suffix(')').unwrap_or(rest);
    let number = |key: &str| -> Option<u64> {
        let at = rest.find(&format!("{key} "))? + key.len() + 1;
        rest[at..].split([' ', ')']).next()?.parse().ok()
    };
    let flags = rest
        .find("FLAGS (")
        .and_then(|at| rest[at + 7..].split(')').next())
        .unwrap_or_default();
    let at = rest
        .find("INTERNALDATE \"")
        .and_then(|at| rest[at + 14..].split('"').next())
        .and_then(|date| chrono::DateTime::parse_from_str(date.trim(), "%d-%b-%Y %H:%M:%S %z").ok())
        .map_or(0, |date| date.timestamp());
    Some(Meta {
        uid: u32::try_from(number("UID")?).ok()?,
        seen: flags.split_whitespace().any(|f| f == "\\Seen"),
        msgid: number("X-GM-MSGID").unwrap_or(0),
        thrid: number("X-GM-THRID").unwrap_or(0),
        at,
    })
}

/// What the start of a message says.
#[derive(Debug, Default, PartialEq)]
struct Message {
    name: String,
    address: String,
    subject: String,
    content_type: String,
    snippet: String,
}

fn parse_message(raw: &str) -> Message {
    let (headers, body) = split_head(raw);
    let header = |name: &str| header_value(&headers, name).unwrap_or_default();
    let (name, address) = parse_address(&decode_words(&header("from")));
    let content_type = header("content-type");
    let text = body_text(&content_type, &header("content-transfer-encoding"), body, 0);
    Message {
        name,
        address,
        subject: decode_words(&header("subject")),
        snippet: snippet(&text),
        content_type,
    }
}

/// Headers (unfolded, as name in lower case and value) and the body.
fn split_head(raw: &str) -> (Vec<(String, String)>, &str) {
    let raw = raw.trim_start_matches(['\r', '\n']);
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .unwrap_or((raw, ""));
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    (headers, body)
}

fn header_value(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
}

/// `"Ayşe Y." <ayse@example.com>` → ("Ayşe Y.", "ayse@example.com")
fn parse_address(from: &str) -> (String, String) {
    match from.rsplit_once('<') {
        Some((name, rest)) => (
            name.trim().trim_matches('"').trim().to_owned(),
            rest.trim_end_matches('>').trim().to_owned(),
        ),
        None => (String::new(), from.trim().to_owned()),
    }
}

/// A parameter of a header value: `boundary` of `multipart/mixed; boundary="x"`.
fn parameter(value: &str, name: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|part| {
        let (key, value) = part.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

/// The readable text of a body: the plain text part of a multipart one, or HTML made
/// plain. `depth` stops messages nested without end.
fn body_text(content_type: &str, encoding: &str, body: &str, depth: u8) -> String {
    let kind = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if kind.starts_with("multipart/") {
        let Some(boundary) = parameter(content_type, "boundary") else {
            return String::new();
        };
        if depth > 4 {
            return String::new();
        }
        let delimiter = format!("--{boundary}");
        let parts: Vec<(Vec<(String, String)>, &str)> =
            body.split(&delimiter).skip(1).map(split_head).collect();
        let text_of = |(headers, body): &(Vec<(String, String)>, &str)| {
            let kind = header_value(headers, "content-type").unwrap_or("text/plain".into());
            let encoding = header_value(headers, "content-transfer-encoding").unwrap_or_default();
            body_text(&kind, &encoding, body, depth + 1)
        };
        let is = |part: &&(Vec<(String, String)>, &str), wanted: &str| {
            header_value(&part.0, "content-type")
                .unwrap_or("text/plain".into())
                .to_ascii_lowercase()
                .starts_with(wanted)
        };
        // Plain text first, then HTML, then whatever nested part has text.
        for wanted in ["text/plain", "text/html", "multipart/"] {
            if let Some(text) = parts
                .iter()
                .filter(|p| is(p, wanted))
                .map(text_of)
                .find(|t| !t.trim().is_empty())
            {
                return text;
            }
        }
        return String::new();
    }
    if !kind.is_empty() && !kind.starts_with("text/") {
        return String::new();
    }
    let bytes = match encoding.trim().to_ascii_lowercase().as_str() {
        "base64" => decode_base64(body),
        "quoted-printable" => decode_quoted_printable(body),
        _ => body.as_bytes().to_vec(),
    };
    let charset = parameter(content_type, "charset").unwrap_or_default();
    let text = decode_charset(&bytes, &charset);
    if kind == "text/html" {
        html_text(&text)
    } else {
        text
    }
}

/// Base64 cut anywhere (the message is downloaded only in part): what decodes.
fn decode_base64(text: &str) -> Vec<u8> {
    let clean: String = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        .collect();
    let whole = clean.len() / 4 * 4;
    base64::engine::general_purpose::STANDARD
        .decode(&clean[..whole])
        .unwrap_or_default()
}

fn decode_quoted_printable(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            // A soft line break
            if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let Some(byte) = bytes
                .get(i + 1..i + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// UTF-8, or the Latin and Turkish single-byte charsets; others as well as they go.
fn decode_charset(bytes: &[u8], charset: &str) -> String {
    let charset = charset.to_ascii_lowercase();
    let turkish = |b: u8| match b {
        0xd0 => 'Ğ',
        0xdd => 'İ',
        0xde => 'Ş',
        0xf0 => 'ğ',
        0xfd => 'ı',
        0xfe => 'ş',
        _ => b as char,
    };
    match charset.as_str() {
        "iso-8859-9" | "windows-1254" | "latin5" => bytes.iter().map(|&b| turkish(b)).collect(),
        "iso-8859-1" | "windows-1252" | "us-ascii" | "latin1" => {
            bytes.iter().map(|&b| b as char).collect()
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// RFC 2047 encoded words in headers: `=?UTF-8?B?...?=` and `=?utf-8?Q?...?=`.
fn decode_words(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let between = &rest[..start];
        let Some(word) = encoded_word(&rest[start..]) else {
            out.push_str(&rest[..start + 2]);
            rest = &rest[start + 2..];
            last_was_word = false;
            continue;
        };
        // Space between two encoded words is not part of the text.
        if !(last_was_word && between.trim().is_empty()) {
            out.push_str(between);
        }
        out.push_str(&word.0);
        rest = &rest[start + word.1..];
        last_was_word = true;
    }
    out.push_str(rest);
    out
}

/// One encoded word at the start of `text`: its text and its length.
fn encoded_word(text: &str) -> Option<(String, usize)> {
    let inner = text.strip_prefix("=?")?;
    let (charset, inner) = inner.split_once('?')?;
    let (encoding, inner) = inner.split_once('?')?;
    let end = inner.find("?=")?;
    let data = &inner[..end];
    let bytes = match encoding.to_ascii_uppercase().as_str() {
        "B" => base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?,
        "Q" => decode_quoted_printable(&data.replace('_', " ")),
        _ => return None,
    };
    let length = 2 + charset.len() + 1 + encoding.len() + 1 + end + 2;
    // A language tag: utf-8*tr
    let charset = charset.split('*').next().unwrap_or_default();
    Some((decode_charset(&bytes, charset), length))
}

/// HTML without tags, styles, scripts and entities.
fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tag = &rest[open..];
        let lower: String = tag.chars().take(8).collect::<String>().to_ascii_lowercase();
        let skip_to = if lower.starts_with("<style") {
            tag.to_ascii_lowercase().find("</style>").map(|i| i + 8)
        } else if lower.starts_with("<script") {
            tag.to_ascii_lowercase().find("</script>").map(|i| i + 9)
        } else {
            tag.find('>').map(|i| i + 1)
        };
        match skip_to {
            Some(len) => {
                out.push(' ');
                rest = &tag[len..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&zwnj;", "")
}

/// The text on one line, cut at a word.
fn snippet(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= SNIPPET_CHARS {
        return line;
    }
    let cut: String = line.chars().take(SNIPPET_CHARS).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_search_results() {
        assert_eq!(parse_search("* SEARCH 4 7 12\r\n"), vec![4, 7, 12]);
        assert_eq!(parse_search("* SEARCH\r\n"), Vec::<u32>::new());
    }

    #[test]
    fn writes_uid_sets() {
        assert_eq!(uid_set(&[7, 1, 2, 3, 9, 10]), "1:3,7,9:10");
        assert_eq!(uid_set(&[5]), "5");
    }

    #[test]
    fn reads_fetched_flags() {
        let text = "* 3 FETCH (X-GM-THRID 1878676773896889013 X-GM-MSGID 1878676775465756366 UID 412 FLAGS (\\Seen \\Answered) INTERNALDATE \"10-Oct-2026 15:23:09 +0000\")\r\n\
                    * 4 FETCH (X-GM-THRID 1 X-GM-MSGID 2 UID 413 FLAGS () INTERNALDATE \" 3-Oct-2026 08:00:00 +0300\")\r\n";
        let metas = parse_fetch(text);
        assert_eq!(
            metas[0],
            Meta {
                uid: 412,
                seen: true,
                msgid: 1878676775465756366,
                thrid: 1878676773896889013,
                at: 1791645789,
            }
        );
        assert!(!metas[1].seen);
        assert_eq!(metas[1].at, 1791003600);
        assert_eq!(format!("{:x}", metas[0].msgid), "1a126692d969a6ce");
    }

    #[test]
    fn links_to_gmail() {
        assert_eq!(
            gmail_link("info@arclude.com", 1878676773896889013, 1878676775465756366),
            "https://mail.google.com/mail/?authuser=info@arclude.com#all/thread-f:1878676773896889013%7Cmsg-f:1878676775465756366"
        );
    }

    #[test]
    fn decodes_encoded_words() {
        assert_eq!(
            decode_words("=?UTF-8?B?VGVrbGlmIGfDtm5kZXJpbGRp?="),
            "Teklif gönderildi"
        );
        assert_eq!(
            decode_words("=?utf-8?Q?Toplant=C4=B1_saati?= =?utf-8?Q?_de=C4=9Fi=C5=9Fti?="),
            "Toplantı saati değişti"
        );
        assert_eq!(
            decode_words("Re: =?ISO-8859-9?Q?=DEubat_faturas=FD?= ekte"),
            "Re: Şubat faturası ekte"
        );
        assert_eq!(decode_words("Plain = subject"), "Plain = subject");
    }

    #[test]
    fn spots_bounces() {
        assert!(is_bounce(
            "mailer-daemon@googlemail.com",
            "Delivery Status Notification (Failure)",
            ""
        ));
        assert!(is_bounce(
            "postmaster@outlook.com",
            "Undeliverable: Teklif",
            ""
        ));
        assert!(is_bounce(
            "noreply@x.com",
            "Hi",
            "multipart/report; report-type=delivery-status; boundary=\"b\""
        ));
        assert!(!is_bounce(
            "ayse@example.com",
            "Teklif hakkında",
            "text/plain"
        ));
    }

    #[test]
    fn reads_a_plain_message() {
        let raw = "From: =?UTF-8?Q?Ay=C5=9Fe_Y=C4=B1lmaz?= <ayse@example.com>\r\n\
                   Subject: Teklif\r\n\
                   Content-Type: text/plain; charset=utf-8\r\n\
                   Content-Transfer-Encoding: quoted-printable\r\n\r\n\
                   Merhaba, teklifinizi ald=C4=B1k.\r\nYar=C4=B1n d=\r\nönerim.\r\n";
        let message = parse_message(raw);
        assert_eq!(message.name, "Ayşe Yılmaz");
        assert_eq!(message.address, "ayse@example.com");
        assert_eq!(message.subject, "Teklif");
        assert_eq!(
            message.snippet,
            "Merhaba, teklifinizi aldık. Yarın dönerim."
        );
    }

    #[test]
    fn reads_the_text_of_a_multipart_message() {
        let raw = "From: shop@example.com\r\n\
                   Subject: Order\r\n\
                   Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n\
                   --outer\r\n\
                   Content-Type: multipart/alternative; boundary=inner\r\n\r\n\
                   --inner\r\n\
                   Content-Type: text/html; charset=utf-8\r\n\r\n\
                   <html><style>p{}</style><p>HTML &amp; more</p></html>\r\n\
                   --inner\r\n\
                   Content-Type: text/plain; charset=utf-8\r\n\
                   Content-Transfer-Encoding: base64\r\n\r\n\
                   U2lwYXJpxZ9pbml6IGhhesSxcg==\r\n\
                   --inner--\r\n\
                   --outer\r\n\
                   Content-Type: application/pdf\r\n\r\nJVBERi0=\r\n";
        let message = parse_message(raw);
        assert_eq!(message.address, "shop@example.com");
        assert_eq!(message.name, "");
        assert_eq!(message.snippet, "Siparişiniz hazır");
    }

    #[test]
    fn makes_html_plain_and_cuts_long_text() {
        let raw = "From: a@b.c\r\nContent-Type: text/html\r\n\r\n<div>Hello&nbsp;<b>there</b></div><script>x()</script>";
        assert_eq!(parse_message(raw).snippet, "Hello there");

        let long = "word ".repeat(100);
        let cut = snippet(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= SNIPPET_CHARS + 1);
    }

    #[test]
    fn decodes_base64_cut_short() {
        // "Merhaba dünya" cut mid-quartet
        assert_eq!(
            decode_base64("TWVyaGFiYSBk\r\nw7xu"),
            "Merhaba dün".as_bytes()
        );
        assert_eq!(
            decode_base64("TWVyaGFiYSBkw7xueW"),
            "Merhaba düny".as_bytes()[..12].to_vec()
        );
    }

    /// Checks a real inbox: `ARNOTCH_MAIL=you@gmail.com cargo test -p arnotch-core
    /// real_inbox -- --ignored --nocapture` (its app password saved in the settings).
    #[test]
    #[ignore]
    fn real_inbox() {
        let address = std::env::var("ARNOTCH_MAIL").expect("set ARNOTCH_MAIL");
        let login = Login::load(&address).unwrap();
        let mut inbox = Inbox::default();
        let (mails, _) = inbox.check(&login).unwrap();
        for mail in &mails {
            println!(
                "{} {} {}{} | {} | {} | {}",
                mail.uid,
                mail.id,
                if mail.unread { "unread " } else { "" },
                if mail.bounce { "BOUNCE" } else { "" },
                mail.from,
                mail.subject,
                mail.snippet.chars().take(60).collect::<String>()
            );
        }
        assert!(!mails.is_empty());
        let (again, arrived) = inbox.check(&login).unwrap();
        assert_eq!(again.len(), mails.len());
        assert!(arrived.is_empty());
    }

    #[test]
    fn splits_downloads() {
        let out = format!("one{END_MARK}two\r\n{END_MARK}");
        assert_eq!(split_transfers(&out), vec!["one", "two\r\n"]);
    }
}
