use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
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

/// Thread-safe weather data shared between fetcher and renderer
struct WeatherData {
    condition: WeatherCondition,
    temperature: f32,
    location: Option<(f64, f64)>,
    last_fetch_attempt: Option<Instant>,
    fetch_in_progress: bool,
}

pub struct WeatherCache {
    pub condition: WeatherCondition,
    pub temperature: f32,
    shared: Arc<Mutex<WeatherData>>,
    fetch_interval: Duration,
    retry_interval: Duration, // Wait before retrying after failure
}

impl WeatherCache {
    pub fn new() -> Self {
        Self {
            condition: WeatherCondition::Unknown,
            temperature: 0.0,
            shared: Arc::new(Mutex::new(WeatherData {
                condition: WeatherCondition::Unknown,
                temperature: 0.0,
                location: None,
                last_fetch_attempt: None,
                fetch_in_progress: false,
            })),
            fetch_interval: Duration::from_secs(3600), // 1 hour between successful fetches
            retry_interval: Duration::from_secs(300),  // 5 minutes between retries on failure
        }
    }

    /// Non-blocking update - spawns background thread if needed, reads cached value
    pub fn update(&mut self) {
        // First, read any updated data from background thread
        if let Ok(data) = self.shared.lock() {
            self.condition = data.condition;
            self.temperature = data.temperature;
        }

        // Check if we should start a new fetch
        let should_fetch = {
            if let Ok(data) = self.shared.lock() {
                if data.fetch_in_progress {
                    false // Already fetching
                } else if let Some(last) = data.last_fetch_attempt {
                    // Use retry_interval if we haven't successfully fetched yet
                    let interval = if data.location.is_some() && data.condition != WeatherCondition::Unknown {
                        self.fetch_interval
                    } else {
                        self.retry_interval
                    };
                    last.elapsed() >= interval
                } else {
                    true // Never fetched
                }
            } else {
                false
            }
        };

        if should_fetch {
            self.spawn_fetch();
        }
    }

    fn spawn_fetch(&self) {
        // Mark fetch in progress
        if let Ok(mut data) = self.shared.lock() {
            if data.fetch_in_progress {
                return; // Another thread already started
            }
            data.fetch_in_progress = true;
            data.last_fetch_attempt = Some(Instant::now());
        }

        let shared = Arc::clone(&self.shared);
        thread::spawn(move || {
            // Get location if needed
            let location = {
                let data = shared.lock().ok();
                data.and_then(|d| d.location)
            };

            let location = location.or_else(fetch_location);

            // Fetch weather if we have location
            let result = location.and_then(|(lat, lon)| {
                fetch_weather(lat, lon).map(|(cond, temp)| (lat, lon, cond, temp))
            });

            // Update shared state
            if let Ok(mut data) = shared.lock() {
                data.fetch_in_progress = false;
                if let Some((lat, lon, condition, temperature)) = result {
                    data.location = Some((lat, lon));
                    data.condition = condition;
                    data.temperature = temperature;
                }
                // last_fetch_attempt already set, so retry logic will use appropriate interval
            }
        });
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
