//! The weather where the user is, from Open-Meteo (free, no account or key): the city
//! named in the settings is looked up once, then the forecast is downloaded every 20 minutes.

use std::process::Command;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

/// The weather now and today's range.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Weather {
    /// The place found for the city in the settings, e.g. "Kadıköy, İstanbul".
    pub place: String,
    /// °C
    pub temperature: f64,
    pub high: f64,
    pub low: f64,
    /// WMO weather code: 0 clear, 1–3 cloudy, 45/48 fog, 51–67 rain, 71–77 snow,
    /// 80–82 showers, 85/86 snow showers, 95–99 thunderstorm.
    pub code: u32,
    pub is_day: bool,
}

/// A place to get the weather for.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

fn get(url: &str, params: &[(&str, &str)]) -> anyhow::Result<serde_json::Value> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--fail",
        "--get",
        "--compressed",
    ])
    .args(["--max-time", "20", "--user-agent", "arnotch"]);
    for (key, value) in params {
        cmd.arg("--data-urlencode").arg(format!("{key}={value}"));
    }
    let out = cmd.arg(url).output().context("cannot run curl")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// Finds a city (or town, district) by name, in the user's language when Open-Meteo has it.
/// "Kadıköy, İstanbul" picks the Kadıköy in İstanbul.
pub fn find_place(query: &str, language: &str) -> anyhow::Result<Place> {
    let (name, region) = match query.split_once(',') {
        Some((name, region)) => (name.trim(), region.trim()),
        None => (query.trim(), ""),
    };
    let reply = get(
        "https://geocoding-api.open-meteo.com/v1/search",
        &[
            ("name", name),
            ("count", "10"),
            ("language", language),
            ("format", "json"),
        ],
    )?;
    parse_place(&reply, name, region).with_context(|| format!("no place called “{query}”"))
}

/// The best of the search results: one named exactly as asked (in `region` when given),
/// the most populous first; the search's own first pick otherwise. The search ranks
/// loosely: "Kadıköy" first finds Babadağ, Denizli.
fn parse_place(reply: &serde_json::Value, name: &str, region: &str) -> Option<Place> {
    let results = reply.get("results")?.as_array()?;
    let text = |r: &serde_json::Value, key: &str| {
        fold(r.get(key).and_then(|v| v.as_str()).unwrap_or_default())
    };
    let (name, region) = (fold(name), fold(region));
    let in_region = |r: &&serde_json::Value| {
        region.is_empty()
            || ["admin1", "admin2", "admin3", "country"]
                .iter()
                .any(|key| text(r, key).contains(&region))
    };
    let population =
        |r: &serde_json::Value| r.get("population").and_then(|p| p.as_u64()).unwrap_or(0);
    let found = results
        .iter()
        .filter(in_region)
        .filter(|r| text(r, "name") == name)
        .max_by_key(|r| population(r))
        .or_else(|| results.iter().find(in_region))?;

    let name = found.get("name")?.as_str()?;
    // "Kadıköy, İstanbul"; a city that is its own province is named once.
    let name = match found.get("admin1").and_then(|a| a.as_str()) {
        Some(region) if region != name => format!("{name}, {region}"),
        _ => name.to_owned(),
    };
    Some(Place {
        name,
        latitude: found.get("latitude")?.as_f64()?,
        longitude: found.get("longitude")?.as_f64()?,
    })
}

/// Lowercase for comparing names: "İstanbul" and "istanbul" match (Unicode lowercases
/// "İ" to "i" with a combining dot).
fn fold(text: &str) -> String {
    text.to_lowercase().replace('\u{307}', "")
}

/// The weather at `place` now.
pub fn current(place: &Place) -> anyhow::Result<Weather> {
    let reply = get(
        "https://api.open-meteo.com/v1/forecast",
        &[
            ("latitude", &place.latitude.to_string()),
            ("longitude", &place.longitude.to_string()),
            ("current", "temperature_2m,weather_code,is_day"),
            ("daily", "temperature_2m_max,temperature_2m_min"),
            ("forecast_days", "1"),
            ("timezone", "auto"),
        ],
    )?;
    parse_weather(&reply, &place.name).context("unexpected reply from Open-Meteo")
}

fn parse_weather(reply: &serde_json::Value, place: &str) -> Option<Weather> {
    let current = reply.get("current")?;
    let daily = reply.get("daily")?;
    let first = |key: &str| daily.get(key)?.get(0)?.as_f64();
    Some(Weather {
        place: place.to_owned(),
        temperature: current.get("temperature_2m")?.as_f64()?,
        high: first("temperature_2m_max")?,
        low: first("temperature_2m_min")?,
        code: current.get("weather_code")?.as_u64()? as u32,
        is_day: current.get("is_day")?.as_u64()? == 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_place() {
        let place = |name: &str, admin1: &str, population: u64| json!({ "name": name, "admin1": admin1, "latitude": 1.0, "longitude": 2.0, "population": population });
        let reply = json!({ "results": [
            place("Babadağ", "Denizli", 4587),
            place("Kadıköy", "Yalova", 2741),
            place("Kadıköy", "Aydın", 1752),
        ] });
        assert_eq!(
            parse_place(&reply, "Kadıköy", "").unwrap().name,
            "Kadıköy, Yalova"
        );
        assert_eq!(
            parse_place(&reply, "kadıköy", "aydın").unwrap().name,
            "Kadıköy, Aydın"
        );
        // No exact name: the search's first pick.
        assert_eq!(
            parse_place(&reply, "Kadikoy", "").unwrap().name,
            "Babadağ, Denizli"
        );
        let reply = json!({ "results": [place("İstanbul", "İstanbul", 15701602)] });
        assert_eq!(
            parse_place(&reply, "istanbul", "").unwrap().name,
            "İstanbul"
        );
        assert_eq!(
            parse_place(&json!({ "generationtime_ms": 0.5 }), "x", ""),
            None
        );
    }

    #[test]
    fn reads_the_weather() {
        let reply = json!({
            "current": { "temperature_2m": 18.4, "weather_code": 2, "is_day": 1 },
            "daily": { "temperature_2m_max": [21.0], "temperature_2m_min": [12.5] }
        });
        let weather = parse_weather(&reply, "İstanbul").unwrap();
        assert_eq!(weather.temperature, 18.4);
        assert_eq!((weather.high, weather.low), (21.0, 12.5));
        assert_eq!(weather.code, 2);
        assert!(weather.is_day);
    }
}
