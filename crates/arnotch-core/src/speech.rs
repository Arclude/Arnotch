//! Claude's answers read aloud, on this computer: [Piper](https://github.com/rhasspy/piper)
//! turns sentences into speech as the answer is written, and `pw-play` plays it.
//!
//! `arnotchctl speech-voice` downloads Piper and a voice into `~/.local/share/arnotch/piper`.
//! `ARNOTCH_PIPER` and `ARNOTCH_PIPER_VOICE` name others.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::i18n::tr;

/// The voice for each language Piper is downloaded with.
pub fn default_voice() -> &'static str {
    match crate::i18n::language() {
        "tr" => "tr_TR-dfki-medium",
        _ => "en_US-lessac-medium",
    }
}

/// `~/.local/share/arnotch/piper`: Piper itself (in `piper/`) and the voices.
pub fn piper_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("arnotch/piper")
}

/// Piper's program: `ARNOTCH_PIPER`, `piper-tts` (as some distributions name it), or the
/// one `arnotchctl speech-voice` downloaded. (A `piper` on the PATH is often the mouse
/// settings app of the same name.)
pub fn find_piper() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ARNOTCH_PIPER") {
        return Some(PathBuf::from(path));
    }
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("piper-tts"))
            .find(|path| path.is_file())
    });
    on_path.or_else(|| Some(piper_dir().join("piper/piper")).filter(|path| path.is_file()))
}

/// The voice: `ARNOTCH_PIPER_VOICE`, or one downloaded, in the user's language if there
/// is one.
pub fn find_voice() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ARNOTCH_PIPER_VOICE") {
        return Some(PathBuf::from(path));
    }
    let mut voices: Vec<PathBuf> = std::fs::read_dir(piper_dir())
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "onnx"))
        .collect();
    voices.sort();
    let language = format!("{}_", crate::i18n::language());
    let mine = voices.iter().position(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(&language))
    });
    match mine {
        Some(at) => Some(voices.swap_remove(at)),
        None => voices.into_iter().next(),
    }
}

/// The voice's sample rate, from the JSON file beside it.
fn sample_rate(voice: &std::path::Path) -> u32 {
    let mut config = voice.as_os_str().to_owned();
    config.push(".json");
    std::fs::read_to_string(config)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json["audio"]["sample_rate"].as_u64())
        .map_or(22050, |rate| rate as u32)
}

/// Speaks sentences one after the other while more are written.
pub struct Speaker {
    piper: Child,
    play: Child,
    input: Option<ChildStdin>,
}

impl Speaker {
    pub fn new() -> anyhow::Result<Self> {
        let piper = find_piper().ok_or_else(|| {
            anyhow::anyhow!(tr("No voice to read answers: run arnotchctl speech-voice"))
        })?;
        let voice = find_voice().ok_or_else(|| {
            anyhow::anyhow!(tr("No voice to read answers: run arnotchctl speech-voice"))
        })?;
        let mut piper = Command::new(&piper)
            .arg("--model")
            .arg(&voice)
            .arg("--output-raw")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let audio = piper.stdout.take().expect("stdout is piped");
        let play = Command::new("pw-play")
            .args(["--raw", "--format", "s16", "--channels", "1"])
            .arg(format!("--rate={}", sample_rate(&voice)))
            .args(["-P", "{ application.name = \"arnotch\" media.name = \"Claude\" media.role = \"Accessibility\" }", "-"])
            .stdin(audio)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let play = match play {
            Ok(play) => play,
            Err(err) => {
                _ = piper.kill();
                _ = piper.wait();
                return Err(anyhow::anyhow!("pw-play: {err}"));
            }
        };
        let input = piper.stdin.take();
        Ok(Speaker { piper, play, input })
    }

    /// The processes to stop it from elsewhere.
    pub fn pids(&self) -> [u32; 2] {
        [self.piper.id(), self.play.id()]
    }

    /// Queues a sentence (Piper reads one per line).
    pub fn say(&mut self, sentence: &str) -> std::io::Result<()> {
        let line = sentence.replace(['\n', '\r'], " ");
        match &mut self.input {
            Some(input) => {
                input.write_all(line.as_bytes())?;
                input.write_all(b"\n")?;
                input.flush()
            }
            None => Ok(()),
        }
    }

    /// Waits until everything said has been played.
    pub fn finish(mut self) {
        self.input.take();
        _ = self.piper.wait();
        _ = self.play.wait();
    }
}

/// Stops a speaker running on another thread, by its [`Speaker::pids`].
pub fn silence(pids: [u32; 2]) {
    for pid in pids {
        // SAFETY: plain system call; at worst the process is already gone.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    }
}

/// Cuts an answer being written into sentences to speak, leaving out what doesn't read
/// aloud: code blocks, Markdown marks, links' addresses, emoji.
#[derive(Default)]
pub struct Sentences {
    pending: String,
    /// Inside a ``` block
    in_code: bool,
}

impl Sentences {
    /// The sentences the new text completes.
    pub fn push(&mut self, text: &str) -> Vec<String> {
        self.pending.push_str(text);
        let mut out = Vec::new();
        while let Some(end) = sentence_end(&self.pending) {
            let sentence: String = self.pending.drain(..end).collect();
            self.take(&sentence, &mut out);
        }
        out
    }

    /// The rest, once the answer is complete.
    pub fn finish(&mut self) -> Vec<String> {
        let rest = std::mem::take(&mut self.pending);
        let mut out = Vec::new();
        self.take(&rest, &mut out);
        out
    }

    fn take(&mut self, text: &str, out: &mut Vec<String>) {
        for line in text.split('\n') {
            if line.trim_start().starts_with("```") {
                self.in_code = !self.in_code;
                continue;
            }
            if self.in_code {
                continue;
            }
            let spoken = speakable(line);
            if spoken.chars().any(char::is_alphanumeric) {
                out.push(spoken);
            }
        }
    }
}

/// Where the first complete sentence ends (after its punctuation), or a line does.
fn sentence_end(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c == '\n' {
            return Some(at + 1);
        }
        if matches!(c, '.' | '!' | '?' | '…' | ':' | ';')
            && let Some(&(next_at, next)) = chars.peek()
            && next.is_whitespace()
        {
            return Some(next_at);
        }
    }
    None
}

/// The line as it should be heard.
fn speakable(line: &str) -> String {
    let mut text = line.trim();
    // Headings, quotes and list marks
    text = text.trim_start_matches(['#', '>', '-', '*', '+', ' ']);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        // [text](address) reads as its text
        if rest.starts_with('[')
            && let Some(close) = rest.find("](")
            && let Some(end) = rest[close..].find(')')
        {
            out.push_str(&rest[1..close]);
            rest = &rest[close + end + 1..];
            continue;
        }
        // Bare addresses are skipped.
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest = &rest[end..];
            continue;
        }
        let c = rest.chars().next().expect("not empty");
        if !matches!(c, '*' | '_' | '`' | '#' | '|' | '~') && !is_emoji(c) {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0xFE0F | 0x200D)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_sentences_as_they_come() {
        let mut sentences = Sentences::default();
        assert_eq!(sentences.push("Merhaba! Yarın hava"), ["Merhaba!"]);
        assert_eq!(
            sentences.push(" 18,5 derece. Şemsiye"),
            ["Yarın hava 18,5 derece."]
        );
        assert!(sentences.push(" gerekmez").is_empty());
        assert_eq!(sentences.finish(), ["Şemsiye gerekmez"]);
    }

    #[test]
    fn numbers_are_not_sentence_ends() {
        let mut sentences = Sentences::default();
        assert!(sentences.push("Sürüm 3.5 çıktı").is_empty());
        assert_eq!(sentences.finish(), ["Sürüm 3.5 çıktı"]);
    }

    #[test]
    fn leaves_out_what_does_not_read_aloud() {
        let mut sentences = Sentences::default();
        let said = sentences.push(
            "## Özet\nŞunu çalıştır:\n```sh\nrm -rf build\n```\n- **Kalın** ve `kod` 🎉\nBak: [belgeler](https://x.org) ya da https://y.org adresine.\n",
        );
        assert_eq!(
            said,
            [
                "Özet",
                "Şunu çalıştır:",
                "Kalın ve kod",
                "Bak:",
                "belgeler ya da adresine."
            ]
        );
    }
}
