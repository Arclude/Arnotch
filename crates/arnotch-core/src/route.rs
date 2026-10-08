//! Picks the model and the effort for a chat message, so that "open Spotify" doesn't cost
//! what a code review does: commands and small talk go to Haiku, questions and research to
//! Sonnet, code, debugging and analysis to Opus with more thinking.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::apps::fold;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Model {
    Haiku,
    Sonnet,
    Opus,
    /// Only ever the user's own choice in Claude Code: never picked, never left for Opus.
    Fable,
}

impl Model {
    /// The alias the Claude Code CLI takes (`--model`).
    pub fn alias(self) -> &'static str {
        match self {
            Model::Haiku => "haiku",
            Model::Sonnet => "sonnet",
            Model::Opus => "opus",
            Model::Fable => "fable",
        }
    }

    /// The next stronger model that is picked, if any.
    pub fn stronger(self) -> Option<Model> {
        match self {
            Model::Haiku => Some(Model::Sonnet),
            Model::Sonnet => Some(Model::Opus),
            Model::Opus | Model::Fable => None,
        }
    }

    /// The name shown under the answer.
    pub fn name(self) -> &'static str {
        match self {
            Model::Haiku => "Haiku",
            Model::Sonnet => "Sonnet",
            Model::Opus => "Opus",
            Model::Fable => "Fable",
        }
    }

    fn parse(name: &str) -> Option<Model> {
        match name {
            "haiku" => Some(Model::Haiku),
            "sonnet" => Some(Model::Sonnet),
            "opus" => Some(Model::Opus),
            "fable" => Some(Model::Fable),
            _ => None,
        }
    }

    /// The model of an id or alias ("claude-sonnet-5-5", "opus", "opus[1m]").
    pub fn of_id(id: &str) -> Option<Model> {
        let id = id.to_ascii_lowercase();
        [Model::Haiku, Model::Sonnet, Model::Opus, Model::Fable]
            .into_iter()
            .find(|m| id.contains(m.alias()))
    }
}

/// Where a message goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub model: Model,
    /// `--effort`; none leaves the model's default (Haiku takes none).
    pub effort: Option<&'static str>,
}

impl Route {
    fn of(model: Model) -> Route {
        let effort = match model {
            Model::Haiku => None,
            Model::Sonnet => Some("medium"),
            Model::Opus | Model::Fable => Some("high"),
        };
        Route { model, effort }
    }
}

/// Words (folded, matched as word beginnings, Turkish and English) that call for Opus.
const HARD: &[&str] = &[
    "kod",
    "code",
    "script",
    "betik",
    "fonksiyon",
    "function",
    "hata",
    "bug",
    "debug",
    "derle",
    "compile",
    "rust",
    "python",
    "javascript",
    "typescript",
    "sql",
    "regex",
    "algoritma",
    "algorithm",
    "analiz",
    "analy",
    "mimari",
    "architect",
    "tasarla",
    "design",
    "strateji",
    "strategy",
    "ispat",
    "kanitla",
    "proof",
    "prove",
    "matematik",
    "optimiz",
    "refactor",
    "detayli",
    "ayrintili",
    "derinlemesine",
    "detailed",
    "indepth",
    "adimadim",
    "stepbystep",
];

/// Words that call for at least Sonnet: research, explanations, writing.
const THINK: &[&str] = &[
    "arastir",
    "research",
    "karsilastir",
    "compare",
    "kiyasla",
    "oner",
    "tavsiye",
    "recommend",
    "suggest",
    "neden",
    "niye",
    "why",
    "nasil",
    "how",
    "acikla",
    "explain",
    "ozetle",
    "ozet",
    "summar",
    "cevir",
    "translat",
    "incele",
    "review",
    "yorumla",
    "fark",
    "differen",
    "yaz",
    "write",
    "hazirla",
    "prepare",
    "listele",
];

/// Words of commands and small talk, which Haiku does well.
const LIGHT: &[&str] = &[
    "ac",
    "kapat",
    "cal",
    "oynat",
    "durdur",
    "duraklat",
    "sustur",
    "ses",
    "parlaklik",
    "zamanlayici",
    "alarm",
    "odak",
    "kilitle",
    "saat",
    "tarih",
    "hava",
    "merhaba",
    "selam",
    "tesekkur",
    "sagol",
    "eyvallah",
    "tamam",
    "open",
    "close",
    "play",
    "pause",
    "stop",
    "mute",
    "volume",
    "brightness",
    "timer",
    "lock",
    "launch",
    "time",
    "date",
    "weather",
    "hello",
    "thanks",
    // Whole words only (see `pick`).
    "hi",
    "ok",
];

/// Words saying the last answer didn't work: the next one goes to a stronger model.
const COMPLAINT: &[&str] = &[
    "olmadi",
    "calismadi",
    "yanlis",
    "hatali",
    "yapamadin",
    "tekrardene",
    "yinedene",
    "anlamadin",
    "wrong",
    "didntwork",
    "doesntwork",
    "notworking",
    "tryagain",
    "stillbroken",
];

/// Words of an answer giving up.
const GIVING_UP: &[&str] = &[
    "yapamiyorum",
    "yapamadim",
    "bulamadim",
    "basaramadim",
    "emindegilim",
    "bilmiyorum",
    "erisimimyok",
    "icant",
    "icannot",
    "icouldnt",
    "unableto",
    "imnotsure",
    "idontknow",
    "idonthaveaccess",
];

/// Words that start like THINK words but are small talk.
const NOT_THINK: &[&str] = &["nasilsin", "howareyou", "yazik"];

/// The route for `prompt` with these attachments. `choice` is the user's setting: "auto",
/// or a model to always use. `previous` is the model of the conversation's last message:
/// a short follow-up ("go on", "and the second one?") stays with it.
pub fn pick(
    choice: &str,
    prompt: &str,
    attachments: &[impl AsRef<Path>],
    previous: Option<Model>,
) -> Route {
    if let Some(model) = Model::parse(choice) {
        return Route::of(model);
    }
    let words: Vec<String> = prompt
        .split_whitespace()
        .map(fold)
        .filter(|w| !w.is_empty())
        .collect();
    // Phrases too, joined ("adım adım", "step by step", "how are you").
    let mut all = words.clone();
    for n in [2, 3] {
        all.extend(words.windows(n).map(|w| w.concat()));
    }
    let starts = |list: &[&str]| all.iter().any(|w| list.iter().any(|k| w.starts_with(k)));
    let small_talk = all.iter().any(|w| NOT_THINK.contains(&w.as_str()));
    // "hi" and "ok" only as whole words, not as the start of "hikaye" or "okul".
    let light = words.iter().any(|w| ["hi", "ok"].contains(&w.as_str()))
        || starts(&LIGHT[..LIGHT.len() - 2]);
    let chars = prompt.chars().count();

    // The last answer didn't do: one step up from it, and at least Sonnet.
    if starts(COMPLAINT) {
        let up = previous.map_or(Model::Sonnet, |p| p.stronger().unwrap_or(p));
        return Route::of(up.max(Model::Sonnet));
    }

    let code_attached = attachments.iter().any(|p| is_code(p.as_ref()));
    if starts(HARD) || chars > 600 || code_attached {
        return Route::of(Model::Opus);
    }
    let think = starts(THINK) && !small_talk;
    if think || !attachments.is_empty() || chars > 160 {
        return Route::of(previous.map_or(Model::Sonnet, |p| p.max(Model::Sonnet)));
    }
    if light || small_talk {
        return Route::of(Model::Haiku);
    }
    // Short and nothing telling: a follow-up keeps the conversation's model.
    Route::of(previous.unwrap_or(if words.len() <= 6 {
        Model::Haiku
    } else {
        Model::Sonnet
    }))
}

/// After an answer from `route`: the route to try again with, when the answer seems not to
/// have done the job (it ended on a failed tool, or says it couldn't) and a stronger model
/// is left. Opus gets no second try.
pub fn escalate(route: Route, last_tool_failed: bool, answer: &str) -> Option<Route> {
    let stronger = route.model.stronger()?;
    // Only the end of the answer: "I couldn't find X, but here is Y" is still an answer.
    let tail: String = answer
        .chars()
        .rev()
        .take(400)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let words: Vec<String> = tail.split_whitespace().map(fold).collect();
    let mut joined = words.clone();
    for n in [2, 3, 4] {
        joined.extend(words.windows(n).map(|w| w.concat()));
    }
    let gives_up = joined
        .iter()
        .any(|w| GIVING_UP.iter().any(|g| w.starts_with(&g.to_lowercase())));
    (last_tool_failed || gives_up).then(|| Route::of(stronger))
}

/// What the router does with a prompt typed to Claude Code in a terminal.
#[derive(Debug, PartialEq, Eq)]
pub enum Terminal {
    /// Let it through to the session's model.
    Keep,
    /// Switch the session to this route, then send the prompt again.
    Switch(Route),
}

/// Sessions with less context than this (tokens) switch down at once: little is cached
/// beyond Claude Code's own prompt, so the cache a new model starts without costs little.
const SMALL_CONTEXT: u64 = 60_000;
/// Otherwise this many light prompts in a row switch down.
const LOWER_STREAK: u32 = 3;

/// Routes a prompt typed to Claude Code. `current` is the session's model, `context` how
/// many tokens its last answer read, `streak` how many prompts in a row wanted a lighter model
/// (updated). Switching models drops the prompt cache, so a session goes up at once but
/// down only while it is short or after a few light prompts; prompts the router can't
/// type again faithfully (several lines, pasted images, commands) go through as they are.
pub fn for_terminal(prompt: &str, current: Model, context: u64, streak: &mut u32) -> Terminal {
    let prompt = prompt.trim();
    let retypable = !prompt.is_empty()
        && !prompt.contains('\n')
        && !prompt.starts_with(['/', '!', '#'])
        && !prompt.contains("[Image")
        && !prompt.contains("[Pasted")
        && prompt.chars().count() <= 2000;
    if !retypable {
        return Terminal::Keep;
    }
    // A terminal session is mostly coding: what reads as a short follow-up stays put.
    let route = pick(
        "auto",
        prompt,
        &[] as &[&str],
        Some(current.min(Model::Opus)),
    );
    // Opus work in a Fable session stays on Fable.
    let wanted = if current == Model::Fable && route.model == Model::Opus {
        current
    } else {
        route.model
    };
    match wanted.cmp(&current) {
        std::cmp::Ordering::Equal => {
            *streak = 0;
            Terminal::Keep
        }
        // Up from anything below Opus; Fable is already above.
        std::cmp::Ordering::Greater => {
            *streak = 0;
            Terminal::Switch(route)
        }
        std::cmp::Ordering::Less => {
            *streak += 1;
            if context < SMALL_CONTEXT || *streak >= LOWER_STREAK {
                *streak = 0;
                Terminal::Switch(route)
            } else {
                Terminal::Keep
            }
        }
    }
}

/// The last answer in a Claude Code transcript (JSONL), from its end: its model, and how
/// many tokens of context it read.
pub fn transcript_model(path: &Path) -> Option<(Model, u64)> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(256 * 1024);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = String::new();
    // A cut-off first line or character only spoils that line.
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    tail.push_str(&String::from_utf8_lossy(&bytes));
    tail.lines().rev().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if value["type"] != "assistant" {
            return None;
        }
        let model = Model::of_id(value["message"]["model"].as_str()?)?;
        let usage = &value["message"]["usage"];
        let context = [
            "input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ]
        .iter()
        .filter_map(|key| usage[key].as_u64())
        .sum();
        Some((model, context))
    })
}

/// Source code and other files whose questions are best left to Opus.
fn is_code(path: &Path) -> bool {
    const CODE: &[&str] = &[
        "rs", "py", "js", "ts", "tsx", "jsx", "c", "h", "cpp", "hpp", "go", "java", "kt", "swift",
        "rb", "php", "sh", "zsh", "qml", "sql", "lua", "cs", "toml", "yaml", "yml", "patch",
        "diff",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CODE.contains(&e.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(prompt: &str) -> Model {
        pick("auto", prompt, &[] as &[&str], None).model
    }

    #[test]
    fn commands_and_small_talk_go_to_haiku() {
        assert_eq!(model("Spotify'ı aç"), Model::Haiku);
        assert_eq!(model("sesi biraz kıs"), Model::Haiku);
        assert_eq!(model("10 dakikalık zamanlayıcı kur"), Model::Haiku);
        assert_eq!(model("merhaba nasılsın"), Model::Haiku);
        assert_eq!(model("open firefox"), Model::Haiku);
        assert_eq!(model("saat kaç"), Model::Haiku);
    }

    #[test]
    fn questions_and_research_go_to_sonnet() {
        assert_eq!(model("en iyi kablosuz kulaklıkları araştır"), Model::Sonnet);
        assert_eq!(model("Wayland ile X11 arasındaki fark ne"), Model::Sonnet);
        assert_eq!(model("bu makaleyi özetle"), Model::Sonnet);
        assert_eq!(model("why is the sky blue"), Model::Sonnet);
        let pdf = ["/tmp/rapor.pdf"];
        assert_eq!(pick("auto", "ne diyor", &pdf, None).model, Model::Sonnet);
    }

    #[test]
    fn code_and_analysis_go_to_opus() {
        assert_eq!(model("bu Rust kodundaki hatayı bul"), Model::Opus);
        assert_eq!(model("bir python betiği yaz"), Model::Opus);
        assert_eq!(model("adım adım anlat"), Model::Opus);
        assert_eq!(model(&"uzun bir soru ".repeat(50)), Model::Opus);
        let code = ["/home/u/main.rs"];
        assert_eq!(
            pick("auto", "bu ne yapıyor", &code, None).model,
            Model::Opus
        );
        assert_eq!(pick("auto", "x", &[] as &[&str], None).effort, None);
        assert_eq!(Route::of(Model::Opus).effort, Some("high"));
    }

    #[test]
    fn follow_ups_keep_the_conversation_model() {
        let none: &[&str] = &[];
        assert_eq!(
            pick("auto", "devam et", none, Some(Model::Opus)).model,
            Model::Opus
        );
        assert_eq!(
            pick("auto", "peki neden", none, Some(Model::Opus)).model,
            Model::Opus
        );
        assert_eq!(
            pick("auto", "peki neden", none, Some(Model::Haiku)).model,
            Model::Sonnet
        );
        // A command after a long talk is still a command.
        assert_eq!(
            pick("auto", "müziği durdur", none, Some(Model::Opus)).model,
            Model::Haiku
        );
    }

    #[test]
    fn complaints_go_up_a_model() {
        let none: &[&str] = &[];
        assert_eq!(
            pick("auto", "olmadı", none, Some(Model::Haiku)).model,
            Model::Sonnet
        );
        assert_eq!(
            pick("auto", "yine çalışmadı", none, Some(Model::Sonnet)).model,
            Model::Opus
        );
        assert_eq!(
            pick("auto", "that's wrong", none, Some(Model::Opus)).model,
            Model::Opus
        );
    }

    #[test]
    fn answers_that_fail_try_again_stronger() {
        let haiku = Route::of(Model::Haiku);
        assert_eq!(
            escalate(haiku, true, "Açtım.").unwrap().model,
            Model::Sonnet
        );
        assert_eq!(
            escalate(haiku, false, "Üzgünüm, bunu yapamıyorum.")
                .unwrap()
                .model,
            Model::Sonnet
        );
        assert_eq!(
            escalate(haiku, false, "Sorry, I couldn't find that app.")
                .unwrap()
                .model,
            Model::Sonnet
        );
        assert_eq!(escalate(haiku, false, "Spotify'ı açtım."), None);
        assert_eq!(escalate(Route::of(Model::Opus), true, "yapamadım"), None);
    }

    #[test]
    fn terminals_go_up_at_once_and_down_slowly() {
        let mut streak = 0;
        let big = 120_000;
        assert_eq!(
            for_terminal(
                "bu fonksiyondaki hatayı düzelt",
                Model::Sonnet,
                big,
                &mut streak
            ),
            Terminal::Switch(Route::of(Model::Opus))
        );
        // A long session stays on Opus for one or two light prompts...
        assert_eq!(
            for_terminal("saat kaç", Model::Opus, big, &mut streak),
            Terminal::Keep
        );
        assert_eq!(
            for_terminal("teşekkürler", Model::Opus, big, &mut streak),
            Terminal::Keep
        );
        // ...and goes down at the third.
        assert!(matches!(
            for_terminal("tamam", Model::Opus, big, &mut streak),
            Terminal::Switch(_)
        ));
        // A short one goes down at once.
        assert!(matches!(
            for_terminal("saat kaç", Model::Opus, 1000, &mut streak),
            Terminal::Switch(_)
        ));
        // Follow-ups, commands and pasted things go through as they are.
        assert_eq!(
            for_terminal("devam et", Model::Opus, big, &mut streak),
            Terminal::Keep
        );
        assert_eq!(
            for_terminal("/model haiku", Model::Opus, 0, &mut streak),
            Terminal::Keep
        );
        assert_eq!(
            for_terminal("satır bir\nsatır iki kod", Model::Haiku, 0, &mut streak),
            Terminal::Keep
        );
        // Fable is never left for Opus.
        assert_eq!(
            for_terminal("bu kodu incele", Model::Fable, 0, &mut streak),
            Terminal::Keep
        );
    }

    #[test]
    fn transcripts_tell_the_model() {
        let path = std::env::temp_dir().join(format!("arnotch-route-{}.jsonl", std::process::id()));
        std::fs::write(&path, concat!(
            r#"{"type":"assistant","message":{"model":"claude-opus-5-5"}}"#, "\n",
            r#"{"type":"user","message":{"content":"x"}}"#, "\n",
            r#"{"type":"assistant","message":{"model":"claude-sonnet-5-5","usage":{"input_tokens":2,"cache_read_input_tokens":36000,"cache_creation_input_tokens":500}}}"#, "\n",
            r#"{"type":"user","message":{"content":"y"}}"#, "\n",
        )).unwrap();
        assert_eq!(transcript_model(&path), Some((Model::Sonnet, 36502)));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(Model::of_id("opus[1m]"), Some(Model::Opus));
    }

    #[test]
    fn a_chosen_model_is_always_used() {
        assert_eq!(
            pick("opus", "selam", &[] as &[&str], None).model,
            Model::Opus
        );
        assert_eq!(
            pick("haiku", "bu kodu incele", &[] as &[&str], None).model,
            Model::Haiku
        );
    }
}
