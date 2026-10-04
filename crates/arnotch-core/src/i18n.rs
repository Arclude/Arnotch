//! Translations of the messages the Rust side shows in the island.
//! (The QML side uses Qt's own translations.)
//!
//! Messages are written in English in the code; [`tr`] looks them up for the user's
//! language, taken from `LC_ALL`, `LC_MESSAGES` or `LANG`. `{}` marks where [`trf`]
//! inserts its arguments, in order.

use std::fmt::Display;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    English,
    Turkish,
}

fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|var| std::env::var(var).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        if locale.starts_with("tr") {
            Lang::Turkish
        } else {
            Lang::English
        }
    })
}

/// The user's language as an ISO 639-1 code ("en", "tr").
pub fn language() -> &'static str {
    match lang() {
        Lang::English => "en",
        Lang::Turkish => "tr",
    }
}

/// The message in the user's language.
pub fn tr(text: &'static str) -> &'static str {
    match lang() {
        Lang::English => text,
        Lang::Turkish => turkish(text).unwrap_or(text),
    }
}

/// [`tr`] with each `{}` replaced by the next argument.
pub fn trf(template: &'static str, args: &[&dyn Display]) -> String {
    fill(tr(template), args)
}

fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut args = args.iter();
    let mut rest = template;
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        if let Some(arg) = args.next() {
            out.push_str(&arg.to_string());
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// A duration such as "2m 05s" or "1h 03m".
pub fn duration(secs: u64) -> String {
    match secs {
        0..60 => trf("{}s", &[&secs]),
        60..3600 => trf("{}m {}s", &[&(secs / 60), &format!("{:02}", secs % 60)]),
        _ => trf(
            "{}h {}m",
            &[&(secs / 3600), &format!("{:02}", secs % 3600 / 60)],
        ),
    }
}

fn turkish(text: &str) -> Option<&'static str> {
    Some(match text {
        // Durations
        "{}s" => "{} sn",
        "{}m {}s" => "{} dk {} sn",
        "{}h {}m" => "{} sa {} dk",
        // Terminals
        "Finished after {} · {} · {}" => "{} sonra bitti · {} · {}",
        "Can't switch to the terminal" => "Terminale geçilemedi",
        "Can't type into the terminal" => "Terminale yazılamadı",
        "Can't send the file to Claude" => "Dosya Claude'a gönderilemedi",
        "Terminal {} is gone" => "{} terminali artık açık değil",
        "Sending text to {} is not supported" => "{} terminaline yazı gönderilemiyor",
        "Konsole blocks typing from other apps. Enable it in Konsole: Settings › Configure Konsole › General › “Enable the security sensitive parts of the DBus API”" => {
            "Konsole başka uygulamaların yazmasını engelliyor. Konsole'da aç: Ayarlar › Konsole'u Yapılandır › Genel › “DBus API'sinin güvenliğe duyarlı kısımlarını etkinleştir”"
        }
        "Arterm's control socket is not available; update Arterm" => {
            "Arterm'in kontrol soketi yok; Arterm'i güncelle"
        }
        "Switching windows is not supported on {} yet" => {
            "{} masaüstünde pencereye geçiş henüz desteklenmiyor"
        }
        "File path copied" => "Dosya yolu kopyalandı",
        "Paste it into Claude with Ctrl+Shift+V" => "Claude'a Ctrl+Shift+V ile yapıştır",
        // Sharing
        "Sharing is off" => "Paylaşım kapalı",
        "Choose a sharing service in the settings" => "Ayarlardan bir paylaşım servisi seç",
        "{} is not a file" => "{} bir dosya değil",
        "Upload failed: {}" => "Yükleme başarısız: {}",
        "no link returned" => "link gelmedi",
        "Cannot run curl" => "curl çalıştırılamadı",
        "No clipboard tool found (install wl-clipboard)" => {
            "Pano aracı bulunamadı (wl-clipboard'u kur)"
        }
        // Claude Code
        "Denied from arnotch" => "arnotch'tan reddedildi",
        // Focus
        "Focusing" => "Odaklanılıyor",
        // Phone
        "{} is not in reach" => "{} erişimde değil",
        "Can't ring the phone" => "Telefon çaldırılamadı",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_arguments_in_order() {
        assert_eq!(
            fill("{} sonra bitti · {}", &[&"2 dk", &"Konsole"]),
            "2 dk sonra bitti · Konsole"
        );
        assert_eq!(fill("no placeholders", &[&1]), "no placeholders");
    }

    #[test]
    fn every_turkish_template_keeps_its_placeholders() {
        for english in [
            "{}s",
            "{}m {}s",
            "{}h {}m",
            "Finished after {} · {} · {}",
            "Terminal {} is gone",
            "Sending text to {} is not supported",
            "Switching windows is not supported on {} yet",
            "{} is not in reach",
            "{} is not a file",
            "Upload failed: {}",
        ] {
            let turkish = turkish(english).unwrap();
            assert_eq!(
                english.matches("{}").count(),
                turkish.matches("{}").count(),
                "{english}"
            );
        }
    }
}
