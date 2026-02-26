use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeatherCondition {
    Sunny,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Thunderstorm,
    Snow,
    Unknown,
}

pub struct WeatherCache {
    pub condition: WeatherCondition,
    pub temperature: f32,
    last_fetch: Option<Instant>,
    fetch_interval: Duration,
    location: Option<(f64, f64)>, // (lat, lon)
}

impl WeatherCache {
    pub fn new() -> Self {
        Self {
            condition: WeatherCondition::Unknown,
            temperature: 0.0,
            last_fetch: None,
            fetch_interval: Duration::from_secs(3600), // 1 hour
            location: None,
        }
    }

    pub fn update(&mut self) {
        // Only fetch if enough time has passed
        if let Some(last) = self.last_fetch {
            if last.elapsed() < self.fetch_interval {
                return;
            }
        }

        // Get location if we don't have it
        if self.location.is_none() {
            self.location = fetch_location();
        }

        // Fetch weather if we have location
        if let Some((lat, lon)) = self.location {
            if let Some((condition, temp)) = fetch_weather(lat, lon) {
                self.condition = condition;
                self.temperature = temp;
                self.last_fetch = Some(Instant::now());
            }
        }
    }
}

fn fetch_location() -> Option<(f64, f64)> {
    // Use ip-api.com for free IP-based geolocation
    let output = Command::new("curl")
        .args(["-s", "--max-time", "5", "http://ip-api.com/json/?fields=lat,lon"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let lat = json.get("lat")?.as_f64()?;
    let lon = json.get("lon")?.as_f64()?;
    Some((lat, lon))
}

fn fetch_weather(lat: f64, lon: f64) -> Option<(WeatherCondition, f32)> {
    // Use Open-Meteo API (free, no key required)
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,weather_code",
        lat, lon
    );

    let output = Command::new("curl")
        .args(["-s", "--max-time", "5", &url])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let current = json.get("current")?;
    
    let weather_code = current.get("weather_code")?.as_i64()? as i32;
    let temperature = current.get("temperature_2m")?.as_f64()? as f32;

    let condition = weather_code_to_condition(weather_code);
    Some((condition, temperature))
}

fn weather_code_to_condition(code: i32) -> WeatherCondition {
    // WMO Weather interpretation codes
    // https://open-meteo.com/en/docs
    match code {
        0 => WeatherCondition::Sunny,                    // Clear sky
        1 | 2 => WeatherCondition::PartlyCloudy,         // Mainly clear, partly cloudy
        3 => WeatherCondition::Cloudy,                   // Overcast
        45 | 48 => WeatherCondition::Fog,                // Fog, depositing rime fog
        51..=57 => WeatherCondition::Drizzle,            // Drizzle (light, moderate, dense)
        61..=65 => WeatherCondition::Rain,               // Rain (slight, moderate, heavy)
        66 | 67 => WeatherCondition::Rain,               // Freezing rain
        71..=77 => WeatherCondition::Snow,               // Snow fall, snow grains
        80..=82 => WeatherCondition::Rain,               // Rain showers
        85 | 86 => WeatherCondition::Snow,               // Snow showers
        95 => WeatherCondition::Thunderstorm,            // Thunderstorm
        96 | 99 => WeatherCondition::Thunderstorm,       // Thunderstorm with hail
        _ => WeatherCondition::Unknown,
    }
}
