//! Asking Claude by voice: the microphone is recorded with `pw-record` until the user
//! stops or falls silent, and whisper.cpp (`whisper-cli`) turns it into text, on this
//! computer.
//!
//! whisper.cpp needs a model: `arnotchctl voice-model` downloads one into
//! `~/.local/share/arnotch/whisper`, or `ARNOTCH_WHISPER_MODEL` names a file.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::i18n::tr;

/// whisper.cpp wants 16 kHz mono.
pub(crate) const RATE: usize = 16_000;
/// The level is measured, and the user's wishes checked, every 50 ms.
pub(crate) const CHUNK: usize = RATE / 20;
/// Falling silent this long after speaking ends the question.
const SILENCE_AFTER_SPEECH: Duration = Duration::from_millis(1600);
/// Not a word this long: nothing is going to be said.
const SILENCE_BEFORE_SPEECH: Duration = Duration::from_secs(8);
/// A question is never longer.
const LONGEST: Duration = Duration::from_secs(90);

/// Models from best to smallest; the best one present is used. (large-v3-turbo-q5_0 is a
/// third the size of large-v3-turbo and, for questions, as good.)
pub const MODELS: &[&str] = &[
    "large-v3-turbo",
    "large-v3-turbo-q5_0",
    "medium",
    "small",
    "base",
    "tiny",
];
/// Models for listening for the wake word, fastest first. (tiny gets too little right,
/// Turkish at least, to be worth its speed.)
const WAKE_MODELS: &[&str] = &["base", "small", "medium", "large-v3-turbo"];

/// What the user wants while listening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Done talking: transcribe what was said.
    Stop,
    /// Forget it.
    Cancel,
}

/// Records the default microphone until `control` says so or the speaker falls silent,
/// calling `on_level` with the loudness (0.0 to 1.0) 20 times a second. Returns the
/// samples, or `None` when cancelled or nothing was said.
pub fn record(
    control: &Receiver<Control>,
    mut on_level: impl FnMut(f32),
) -> anyhow::Result<Option<Vec<i16>>> {
    let mut microphone = Microphone::open("Voice")?;

    let started = Instant::now();
    let mut samples: Vec<i16> = Vec::with_capacity(RATE * 10);
    let mut detector = Speech::default();
    let outcome = loop {
        let piece = match microphone.next_chunk() {
            Ok(piece) => piece,
            Err(err) => break Err(err),
        };
        let loudness = rms(&piece);
        samples.extend_from_slice(&piece);
        on_level(level(loudness));

        match control.try_recv() {
            Ok(Control::Stop) => break Ok(detector.heard()),
            Ok(Control::Cancel) | Err(TryRecvError::Disconnected) => break Ok(false),
            Err(TryRecvError::Empty) => {}
        }
        match detector.feed(loudness, started.elapsed()) {
            Heard::Listening => {}
            Heard::Finished => break Ok(true),
            Heard::Nothing => break Ok(false),
        }
        if started.elapsed() > LONGEST {
            break Ok(true);
        }
    };
    drop(microphone);
    if !outcome? {
        return Ok(None);
    }
    // The pause that ended the question is only more to transcribe.
    let spoken = (detector.last_voice.as_secs_f32() + 0.4) * RATE as f32;
    samples.truncate((spoken as usize).max(RATE / 2));
    Ok(Some(samples))
}

/// The default microphone, recorded with `pw-record` in 50 ms chunks until dropped.
pub(crate) struct Microphone {
    child: std::process::Child,
    stdout: std::process::ChildStdout,
    chunk: Vec<u8>,
}

impl Microphone {
    /// `name` tells the desktop's microphone indicator what arnotch listens for.
    pub(crate) fn open(name: &str) -> anyhow::Result<Self> {
        let mut child = Command::new("pw-record")
            .args(["--raw", "--format", "s16", "--channels", "1"])
            .arg(format!("--rate={RATE}"))
            .args([
                "-P",
                &format!("{{ application.name = \"arnotch\" media.name = \"{name}\" }}"),
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| anyhow::anyhow!("{}: {err}", tr("Can't record (pw-record)")))?;
        let stdout = child.stdout.take().expect("stdout is piped");
        Ok(Self {
            child,
            stdout,
            chunk: vec![0u8; CHUNK * 2],
        })
    }

    /// The next 50 ms of sound; waits for it.
    pub(crate) fn next_chunk(&mut self) -> anyhow::Result<Vec<i16>> {
        self.stdout
            .read_exact(&mut self.chunk)
            .map_err(|err| anyhow::anyhow!("{}: {err}", tr("The recording stopped")))?;
        Ok(self
            .chunk
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect())
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        _ = self.child.kill();
        _ = self.child.wait();
    }
}

/// Root mean square of the samples, from 0.0 to 1.0.
pub(crate) fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Loudness for display: -60 dB is silent, 0 dB is full.
fn level(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

enum Heard {
    Listening,
    /// Spoke, then fell silent.
    Finished,
    /// Never spoke.
    Nothing,
}

/// Tells speech from the room's own noise, which it learns as it goes.
#[derive(Default)]
pub(crate) struct Gate {
    /// The quietest the room has been (smoothed), 0 until measured.
    floor: f32,
    smoothed: f32,
}

impl Gate {
    /// Whether the chunk with this loudness is someone talking.
    pub(crate) fn voice(&mut self, rms: f32) -> bool {
        self.smoothed = if self.smoothed == 0.0 {
            rms
        } else {
            self.smoothed * 0.6 + rms * 0.4
        };
        if self.floor == 0.0 || self.smoothed < self.floor {
            self.floor = self.smoothed.max(1e-4);
        } else {
            // Drifts up slowly, so a fan turning on doesn't count as talking forever.
            self.floor *= 1.002;
        }
        self.smoothed > (self.floor * 3.5).max(0.012)
    }
}

/// Listens for a question: talking, then a pause.
#[derive(Default)]
struct Speech {
    gate: Gate,
    spoke: bool,
    last_voice: Duration,
}

impl Speech {
    fn feed(&mut self, rms: f32, at: Duration) -> Heard {
        if self.gate.voice(rms) {
            self.spoke = true;
            self.last_voice = at;
        }
        if self.spoke && at - self.last_voice > SILENCE_AFTER_SPEECH {
            Heard::Finished
        } else if !self.spoke && at > SILENCE_BEFORE_SPEECH {
            Heard::Nothing
        } else {
            Heard::Listening
        }
    }

    fn heard(&self) -> bool {
        self.spoke
    }
}

/// `~/.local/share/arnotch/whisper`, where `arnotchctl voice-model` puts models.
pub fn models_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("arnotch/whisper")
}

/// The model to use: `ARNOTCH_WHISPER_MODEL`, or the best one downloaded.
pub fn find_model() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ARNOTCH_WHISPER_MODEL") {
        return Some(PathBuf::from(path));
    }
    let dir = models_dir();
    MODELS
        .iter()
        .map(|name| dir.join(model_file(name)))
        .find(|path| path.is_file())
}

pub fn model_file(name: &str) -> String {
    format!("ggml-{name}.bin")
}

/// The model to listen for the wake word with: `ARNOTCH_WAKE_MODEL`, or the fastest one
/// downloaded that is good enough.
pub fn find_wake_model() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ARNOTCH_WAKE_MODEL") {
        return Some(PathBuf::from(path));
    }
    let dir = models_dir();
    WAKE_MODELS
        .iter()
        .map(|name| dir.join(model_file(name)))
        .find(|path| path.is_file())
}

/// Turns the recording into text with whisper.cpp, in whatever language was spoken.
pub fn transcribe(samples: &[i16]) -> anyhow::Result<String> {
    let model = find_model()
        .ok_or_else(|| anyhow::anyhow!(tr("No speech model: run arnotchctl voice-model")))?;
    transcribe_with(&model, samples)
}

/// Turns the recording into text with the whisper.cpp model at `model`.
pub fn transcribe_with(model: &Path, samples: &[i16]) -> anyhow::Result<String> {
    // Numbered: the wake word and a question may be transcribed at the same time.
    static FILES: AtomicU32 = AtomicU32::new(0);
    let n = FILES.fetch_add(1, Ordering::Relaxed);
    let wav = std::env::temp_dir().join(format!("arnotch-voice-{}-{n}.wav", std::process::id()));
    std::fs::write(&wav, wav_bytes(samples))?;
    let output = whisper(model, &wav, samples.len());
    _ = std::fs::remove_file(&wav);
    let output = output?;
    let gpu = on_gpu(&String::from_utf8_lossy(&output.stderr));
    if gpu != GPU.swap(gpu, Ordering::Relaxed) {
        tracing::info!(
            "whisper.cpp runs on the {}",
            if gpu { "GPU" } else { "CPU" }
        );
    }
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let last = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default();
        anyhow::bail!("whisper-cli: {last}");
    }
    let text = clean(&String::from_utf8_lossy(&output.stdout));
    tracing::debug!("{}: {text:?}", model.display());
    Ok(text)
}

/// Tuned for short questions on a CPU, where a 3-second one went from 16 to 1.2 seconds:
/// the language is not guessed (that alone took 3 seconds), the model looks at a 10-second
/// window (or as much as was recorded) instead of a whole 30-second one, and decoding is
/// greedy.
///
/// Fallback stays on: greedy decoding can get stuck repeating a phrase, and whisper escapes
/// that by retrying at a higher temperature. `--no-fallback` made the repeats stick.
fn whisper(model: &Path, wav: &Path, samples: usize) -> anyhow::Result<std::process::Output> {
    // More threads than this only wait on each other.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(6));
    Command::new("whisper-cli")
        .arg("--model")
        .arg(model)
        .arg("--file")
        .arg(wav)
        .args(["--language", &language()])
        .args([
            "--audio-ctx",
            &audio_context(samples, GPU.load(Ordering::Relaxed)).to_string(),
        ])
        .args(["--beam-size", "1", "--best-of", "1", "--flash-attn"])
        .args(["--no-timestamps", "--no-prints"])
        .args(["--threads", &threads.to_string()])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow::anyhow!(tr("whisper.cpp is not installed (whisper-cli)"))
            }
            _ => err.into(),
        })
}

/// The spoken language: `ARNOTCH_WHISPER_LANGUAGE` ("auto" guesses), else the desktop's.
fn language() -> String {
    std::env::var("ARNOTCH_WHISPER_LANGUAGE")
        .ok()
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| crate::i18n::language().to_owned())
}

/// Whether whisper.cpp ran on the graphics card, as the last transcript's log said.
static GPU: AtomicBool = AtomicBool::new(false);

/// Whether whisper.cpp's log says it loaded a backend for a graphics card (Vulkan, CUDA,
/// ROCm...): ggml loads those from `/usr/lib/ggml` when they are installed.
fn on_gpu(log: &str) -> bool {
    log.lines().any(|line| {
        line.strip_prefix("load_backend: loaded ")
            .and_then(|rest| rest.split_whitespace().next())
            .is_some_and(|backend| !matches!(backend, "CPU" | "BLAS"))
    })
}

/// How much of whisper's 30-second window (1500 frames) to look at. On a graphics card the
/// whole window takes no longer, and the text is better for it ("toplantım" stayed
/// "toplantın" with less). On a CPU: what the recording fills with two seconds to spare,
/// but no less than 10 seconds. A smaller window is quicker, but under 10 seconds the text
/// fell apart ("Tamam" became "Tamat. Tamat."), and whisper's retries on what it couldn't
/// read made it slower after all.
fn audio_context(samples: usize, gpu: bool) -> usize {
    if gpu {
        return 1500;
    }
    let frames = samples * 1500 / (RATE * 30);
    (frames + 100).clamp(500, 1500)
}

/// One line of text, without what whisper writes for sounds ("[Music]", "(laughs)").
fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    let mut starred = false;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            '*' => starred = !starred,
            _ if depth == 0 && !starred => out.push(c),
            _ => {}
        }
    }
    drop_repeats(&out)
}

/// Longest phrase (in words) that is checked for repeating.
const MAX_PHRASE: usize = 30;

/// Says a phrase repeated three or more times in a row only once. Whisper sometimes loops
/// on one sentence until it runs out of length; the loop is not a second thought, so it goes.
fn drop_repeats(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    // "Merhaba," and "merhaba" are the same word for this.
    let key = |w: &str| {
        w.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    let mut out: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        // The shortest phrase that starts here and repeats: how long, and how many times.
        let repeat = (1..=MAX_PHRASE.min(words.len() - i) / 3).find_map(|len| {
            let mut times = 1;
            while i + (times + 1) * len <= words.len()
                && (0..len).all(|j| key(words[i + times * len + j]) == key(words[i + j]))
            {
                times += 1;
            }
            (times >= 3).then_some((len, times))
        });
        match repeat {
            Some((len, times)) => {
                out.extend_from_slice(&words[i..i + len]);
                i += len * times;
            }
            None => {
                out.push(words[i]);
                i += 1;
            }
        }
    }
    out.join(" ")
}

/// A 16-bit mono WAV file of the samples.
fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&(RATE as u32).to_le_bytes());
    out.extend_from_slice(&(RATE as u32 * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_the_transcript() {
        assert_eq!(
            clean(" Merhaba,\n nasılsın? [Müzik]\n"),
            "Merhaba, nasılsın?"
        );
        assert_eq!(clean("(gülüyor) Tamam *alkış* oldu"), "Tamam oldu");
        assert_eq!(clean("[BLANK_AUDIO]"), "");
    }

    #[test]
    fn drops_a_phrase_repeated_in_a_loop() {
        assert_eq!(
            drop_repeats("Bu ses bozuk. Bu ses bozuk. Bu ses bozuk. Bu ses bozuk. Bu ses"),
            "Bu ses bozuk. Bu ses"
        );
        assert_eq!(
            drop_repeats("Merhaba, merhaba merhaba. Nasılsın?"),
            "Merhaba, Nasılsın?"
        );
        assert_eq!(
            drop_repeats(&"Şu an dağ sefine geliyor mu? ".repeat(22)),
            "Şu an dağ sefine geliyor mu?"
        );
        // Twice is still a sentence someone said twice.
        assert_eq!(drop_repeats("Evet evet bugün"), "Evet evet bugün");
        assert_eq!(drop_repeats("Yarın saat dokuzda"), "Yarın saat dokuzda");
    }

    #[test]
    fn measures_loudness() {
        assert_eq!(rms(&[]), 0.0);
        assert!((rms(&[16384, -16384]) - 0.5).abs() < 1e-6);
        assert_eq!(level(0.0), 0.0);
        assert_eq!(level(1.0), 1.0);
        assert!((level(0.001) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn hears_speech_then_silence() {
        let mut speech = Speech::default();
        let ms = Duration::from_millis;
        // A quiet room
        for i in 0..10 {
            assert!(matches!(speech.feed(0.002, ms(i * 50)), Heard::Listening));
        }
        // Talking
        for i in 10..30 {
            assert!(matches!(speech.feed(0.1, ms(i * 50)), Heard::Listening));
        }
        assert!(speech.heard());
        // Quiet again: done after the pause.
        let mut finished = None;
        for i in 30..80 {
            if let Heard::Finished = speech.feed(0.002, ms(i * 50)) {
                finished = Some(i);
                break;
            }
        }
        let at = finished.expect("silence ends the question");
        assert!((62..=68).contains(&at), "{at}");
    }

    #[test]
    fn gives_up_when_nothing_is_said() {
        let mut speech = Speech::default();
        let mut gave_up = false;
        for i in 0..200 {
            if let Heard::Nothing = speech.feed(0.003, Duration::from_millis(i * 50)) {
                gave_up = true;
                break;
            }
        }
        assert!(gave_up);
    }

    #[test]
    fn looks_at_as_much_audio_as_recorded() {
        assert_eq!(audio_context(0, false), 500);
        assert_eq!(audio_context(RATE * 3, false), 500);
        assert_eq!(audio_context(RATE * 15, false), 850);
        assert_eq!(audio_context(RATE * 60, false), 1500);
        assert_eq!(audio_context(RATE * 3, true), 1500);
    }

    #[test]
    fn tells_whether_whisper_runs_on_the_gpu() {
        let vulkan = "ggml_vulkan: Found 1 Vulkan devices:\n\
            load_backend: loaded Vulkan backend from /usr/lib/ggml/libggml-vulkan.so\n\
            load_backend: loaded CPU backend from /usr/lib/ggml/libggml-cpu-haswell.so\n";
        assert!(on_gpu(vulkan));
        assert!(on_gpu(
            "load_backend: loaded CUDA backend from /usr/lib/ggml/libggml-cuda.so"
        ));
        assert!(!on_gpu(
            "load_backend: loaded CPU backend from /usr/lib/ggml/libggml-cpu-haswell.so\n\
             read_audio_data: reading audio data"
        ));
        assert!(!on_gpu(""));
    }

    #[test]
    fn writes_a_wav_header() {
        let wav = wav_bytes(&[0, 1, -1]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
    }
}
