//! Waking arnotch by voice: while it is on, the microphone is listened to for a name
//! ("Claude"). Each short burst of speech is turned into text with a fast whisper.cpp
//! model, on this computer; when it starts with the name, arnotch listens for a question,
//! or takes the rest of what was said as the question ("Arnotch, hava nasıl?").
//!
//! whisper writes a made-up name in many ways ("Armoç", "Arnaç", "Or not", "Armut"), so
//! the name is compared by how it sounds (see [`sound`]). It is not given the name as a
//! prompt: with one, it "heard" the name in plain noise.

use std::collections::VecDeque;
use std::path::Path;

use crate::voice::{self, Gate, Microphone};

/// Speech this long before the burst starts is kept: the gate opens a little late.
const PRE_ROLL: usize = 6; // chunks, 300 ms
/// A pause this long ends the burst.
const PAUSE: usize = 10; // chunks, 500 ms
/// Shorter bursts are a cough or a click. (The gate closes some 200 ms after the sound.)
const SHORTEST: usize = 8; // chunks with voice, 400 ms
/// Longer bursts are someone talking, not calling arnotch; they are skipped unheard.
const LONGEST: usize = 7 * 20; // chunks, 7 s

/// Words said before the name that don't count: "Hey Arnotch".
const FILLERS: &[&str] = &[
    "hey", "hei", "hi", "ok", "okay", "hadi", "ya", "ey", "e", "a",
];

/// Listens for `word` until `keep_on` says to stop, calling `on_wake` with the question
/// said along with it (empty when there was none), and `on_clipped` when the microphone is
/// turned up so far that speech is distorted. The microphone is ignored while `paused`
/// (arnotch listens for a question or talks itself).
pub fn listen(
    word: &str,
    model: &Path,
    keep_on: impl Fn() -> bool,
    paused: impl Fn() -> bool,
    mut on_wake: impl FnMut(String),
    mut on_clipped: impl FnMut(),
) -> anyhow::Result<()> {
    let mut microphone = Microphone::open("Wake word")?;
    let mut bursts = Bursts::default();
    let mut chunks: u32 = 0;
    while keep_on() {
        // Every second: another microphone chosen, or a headset plugged in.
        chunks = chunks.wrapping_add(1);
        if chunks.is_multiple_of(20) && microphone.outdated() {
            tracing::info!("wake word: switching microphones");
            microphone = Microphone::open("Wake word")?;
            bursts = Bursts::default();
        }
        let piece = microphone.next_chunk()?;
        if paused() {
            bursts = Bursts::default();
            continue;
        }
        let Some(burst) = bursts.feed(&piece) else {
            continue;
        };
        if voice::clipped(&burst) {
            on_clipped();
        }
        let text = match voice::transcribe_with(model, &burst) {
            Ok(text) => text,
            Err(err) => {
                tracing::warn!("wake word: {err:#}");
                continue;
            }
        };
        let Some(rest) = heard(&text, word) else {
            tracing::debug!("wake word: not it: {text:?}");
            continue;
        };
        tracing::info!("wake word: {text:?}");
        // A question said along with the name is worth the better model.
        let question = if rest.split_whitespace().count() >= 2 {
            voice::transcribe(&burst)
                .ok()
                .map(|text| heard(&text, word).unwrap_or(text))
                .unwrap_or(rest)
        } else {
            String::new()
        };
        on_wake(question);
        bursts = Bursts::default();
    }
    Ok(())
}

/// Cuts the sound into bursts of speech.
#[derive(Default)]
struct Bursts {
    gate: Gate,
    /// The last few chunks before speech.
    before: VecDeque<Vec<i16>>,
    /// The burst so far: chunks, from the first with voice.
    burst: Vec<Vec<i16>>,
    /// Chunks with voice in it, and quiet ones since the last.
    voiced: usize,
    quiet: usize,
    /// Someone talks on and on: nothing is heard until the next pause.
    skipping: bool,
    /// The loudest chunk of the burst, for the log.
    loudest: f32,
}

impl Bursts {
    /// Takes 50 ms of sound; returns a burst of speech once it ends.
    fn feed(&mut self, piece: &[i16]) -> Option<Vec<i16>> {
        let loudness = voice::rms(piece);
        let voice = self.gate.voice(loudness);
        if self.skipping {
            self.quiet = if voice { 0 } else { self.quiet + 1 };
            if self.quiet >= PAUSE {
                self.skipping = false;
                self.quiet = 0;
            }
            return None;
        }
        if self.burst.is_empty() {
            if !voice {
                self.before.push_back(piece.to_vec());
                if self.before.len() > PRE_ROLL {
                    self.before.pop_front();
                }
                return None;
            }
            self.burst.extend(self.before.drain(..));
        }
        self.burst.push(piece.to_vec());
        self.loudest = self.loudest.max(loudness);
        if voice {
            self.voiced += 1;
            self.quiet = 0;
        } else {
            self.quiet += 1;
        }
        if self.voiced > LONGEST {
            tracing::debug!("wake word: skipping long talk");
            self.loudest = 0.0;
            self.burst.clear();
            self.voiced = 0;
            self.quiet = 0;
            self.skipping = true;
            return None;
        }
        if self.quiet < PAUSE {
            return None;
        }
        let burst = std::mem::take(&mut self.burst);
        let voiced = std::mem::take(&mut self.voiced);
        let loudest = std::mem::take(&mut self.loudest);
        self.quiet = 0;
        tracing::debug!(
            "wake word: burst of {} ms, loudest {loudest:.3}{}",
            voiced * 50,
            if voiced < SHORTEST { ", too short" } else { "" }
        );
        if voiced < SHORTEST {
            return None;
        }
        // Without most of the pause, which is only more to transcribe.
        Some(burst[..burst.len() - PAUSE + 4].concat())
    }
}

/// When `text` starts with `word` (after "hey" or so), what was said after it.
pub fn heard(text: &str, word: &str) -> Option<String> {
    let wanted: String = word
        .split_whitespace()
        .filter(|w| !FILLERS.contains(&plain(w).as_str()))
        .map(sound)
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let start = words
        .iter()
        .position(|w| !FILLERS.contains(&plain(w).as_str()))?;
    // The name may come out as one word or split in two ("Or not").
    let parts = word.split_whitespace().count() + 1;
    let mut said = String::new();
    for (i, w) in words.iter().enumerate().skip(start).take(parts) {
        said.push_str(&sound(w));
        if sounds_like(&said, &wanted) {
            let rest = words[i + 1..].join(" ");
            return Some(
                rest.trim_start_matches([',', '.', '!', '?', ' '])
                    .to_owned(),
            );
        }
    }
    None
}

/// The same, or with one sound more: "Arnold" for "Arnotch" ("arnart" for "arnat").
/// One sound less is too much: "Araç" would wake it. Only for names of five sounds or more.
fn sounds_like(said: &str, wanted: &str) -> bool {
    if said == wanted {
        return true;
    }
    let (said, wanted): (Vec<char>, Vec<char>) = (said.chars().collect(), wanted.chars().collect());
    // Short names must match: "Kilit" sounds "krat" with one sound more.
    wanted.len() >= 5
        && said.len() == wanted.len() + 1
        && (0..said.len()).any(|skip| {
            said.iter()
                .enumerate()
                .filter(|&(i, _)| i != skip)
                .map(|(_, c)| c)
                .eq(wanted.iter())
        })
}

/// A word in lower case, without punctuation, Turkish letters as their plain ones.
fn plain(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| {
            Some(match c {
                'ç' => 'c',
                'ş' => 's',
                'ğ' => 'g',
                'ı' => 'i',
                'ö' => 'o',
                'ü' => 'u',
                'â' => 'a',
                'î' => 'i',
                'û' => 'u',
                c if c.is_alphanumeric() => c,
                _ => return None,
            })
        })
        .collect()
}

/// How a word sounds, roughly: every vowel is one, so are m and n, l and r, the sounds of
/// t, d, ç, s, z and j, and so on; repeats are one. "Arnotch", "Armut", "Almut", "Arnaç"
/// and "Or not" all sound "arnat"; "Claude", "Klod" and "Cloud" sound "krat".
pub fn sound(word: &str) -> String {
    let mut word = plain(word);
    // English spelling: a silent e at the end ("Claude" is said "Klod").
    if word.len() >= 5 && word.ends_with('e') && !word.ends_with("ee") {
        word.pop();
    }
    let word = word
        .replace("cl", "kl")
        .replace("cr", "kr")
        .replace("tch", "c")
        .replace("ch", "c")
        .replace("sh", "s")
        .replace("ph", "f")
        .replace("th", "t")
        .replace("ck", "k");
    let mut out = String::new();
    for c in word.chars() {
        let class = match c {
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' => 'a',
            'm' | 'n' => 'n',
            'l' | 'r' => 'r',
            't' | 'd' | 'c' | 'j' | 's' | 'z' | 'x' => 't',
            'b' | 'p' => 'p',
            'k' | 'g' | 'q' => 'k',
            'v' | 'w' | 'f' => 'f',
            'h' => continue,
            c => c,
        };
        if !out.ends_with(class) {
            out.push(class);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::{CHUNK, RATE};

    /// How many 50 ms chunks `seconds` is.
    fn chunks(seconds: f32) -> usize {
        (seconds * RATE as f32 / CHUNK as f32) as usize
    }

    #[test]
    fn hears_the_name_however_whisper_spells_it() {
        for text in [
            "Arnotch.",
            "Armoç",
            "Armoch",
            "Hey Armoç.",
            "Hey Arnold.",
            "Hey Almut.",
            "Arnaç",
            "Ornaç",
            "Or not.",
            "Armut",
            "Arnot.",
            "\"Arnoç\"",
        ] {
            assert_eq!(heard(text, "Arnotch").as_deref(), Some(""), "{text}");
        }
    }

    #[test]
    fn takes_what_follows_as_the_question() {
        assert_eq!(
            heard("Arnoç, bugün hava nasıl?", "Arnotch").as_deref(),
            Some("bugün hava nasıl?")
        );
        assert_eq!(
            heard("Hey Arnold. Müziği aç", "Hey Arnotch").as_deref(),
            Some("Müziği aç")
        );
    }

    #[test]
    fn other_words_do_not_wake_it() {
        for text in [
            "Araç geldi",
            "Bugün akşam ne yiyelim?",
            "Tamam.",
            "Tamar.",
            "(Kalabalık)",
            "Bir şey sorayım, Arnotch",
            "Anne",
            "",
        ] {
            assert_eq!(heard(text, "Arnotch"), None, "{text}");
        }
    }

    #[test]
    fn hears_claude() {
        for text in [
            "Claude.", "Klod", "Klod.", "Hey Klod", "Cloud", "Klot", "Kloud", "Clod",
        ] {
            assert_eq!(heard(text, "Claude").as_deref(), Some(""), "{text}");
        }
        assert_eq!(
            heard("Klod, bugün hava nasıl?", "Claude").as_deref(),
            Some("bugün hava nasıl?")
        );
        for text in [
            "Bulut var.",
            "Kilit açık.",
            "Kolot geldi.",
            "Kalp",
            "Kırk",
            "Lot",
            "",
        ] {
            assert_eq!(heard(text, "Claude"), None, "{text}");
        }
    }

    #[test]
    fn any_name_can_be_chosen() {
        assert_eq!(
            heard("Jarvis, saat kaç?", "Jarvis").as_deref(),
            Some("saat kaç?")
        );
        assert_eq!(heard("Hey Jarvis", "Jarvis").as_deref(), Some(""));
        assert_eq!(heard("Jarvis", "Hey"), None);
    }

    #[test]
    fn sounds() {
        assert_eq!(sound("Arnotch"), "arnat");
        assert_eq!(sound("Armut"), "arnat");
        assert_eq!(sound("ARNAÇ"), "arnat");
        assert_eq!(sound("Arnold"), "arnart");
        assert_eq!(sound("Claude"), "krat");
        assert_eq!(sound("Klod"), "krat");
    }

    fn feed(bursts: &mut Bursts, level: i16, seconds: f32) -> Vec<Vec<i16>> {
        let piece: Vec<i16> = (0..CHUNK)
            .map(|i| if i % 2 == 0 { level } else { -level })
            .collect();
        (0..chunks(seconds))
            .filter_map(|_| bursts.feed(&piece))
            .collect()
    }

    #[test]
    fn cuts_speech_into_bursts() {
        let mut bursts = Bursts::default();
        assert!(feed(&mut bursts, 60, 2.0).is_empty());
        assert!(feed(&mut bursts, 3000, 1.0).is_empty());
        let found = feed(&mut bursts, 60, 1.0);
        assert_eq!(found.len(), 1);
        // The second of speech, a little before it and a little of the pause.
        let seconds = found[0].len() as f32 / RATE as f32;
        assert!((1.3..1.9).contains(&seconds), "{seconds}");
    }

    #[test]
    fn skips_clicks_and_long_talks() {
        let mut bursts = Bursts::default();
        assert!(feed(&mut bursts, 60, 2.0).is_empty());
        // A click
        assert!(feed(&mut bursts, 3000, 0.1).is_empty());
        assert!(feed(&mut bursts, 60, 1.0).is_empty());
        // Talking for ten seconds
        assert!(feed(&mut bursts, 3000, 10.0).is_empty());
        assert!(feed(&mut bursts, 60, 1.0).is_empty());
        // Then the name
        assert!(feed(&mut bursts, 3000, 0.8).is_empty());
        assert_eq!(feed(&mut bursts, 60, 1.0).len(), 1);
    }
}
