use std::fs;
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct MetricsSample {
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub volume_percent: f32,
    pub net_up_bps: f64,
    pub net_down_bps: f64,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
}

#[derive(Default)]
struct CpuSnapshot {
    total: u64,
    idle: u64,
}

#[derive(Default)]
struct NetSnapshot {
    iface: String,
    rx: u64,
    tx: u64,
    at: Option<Instant>,
}

pub struct MetricsCollector {
    last_cpu: Option<CpuSnapshot>,
    last_net: Option<NetSnapshot>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            last_cpu: None,
            last_net: None,
        }
    }

    pub fn sample(&mut self, preferred_iface: Option<&str>) -> MetricsSample {
        let cpu_percent = self.read_cpu_percent();
        let mem_percent = self.read_mem_percent();
        let volume_percent = self.read_volume_percent();
        let (net_down_bps, net_up_bps) = self.read_network_speed(preferred_iface);
        let (caps_lock, num_lock, scroll_lock) = self.read_keyboard_leds();

        MetricsSample {
            cpu_percent,
            mem_percent,
            volume_percent,
            net_up_bps,
            net_down_bps,
            caps_lock,
            num_lock,
            scroll_lock,
        }
    }

    fn read_cpu_percent(&mut self) -> f32 {
        let content = match fs::read_to_string("/proc/stat") {
            Ok(v) => v,
            Err(_) => return 0.0,
        };

        let Some(line) = content.lines().next() else {
            return 0.0;
        };

        let parts: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|p| p.parse::<u64>().ok())
            .collect();

        if parts.len() < 4 {
            return 0.0;
        }

        let idle = parts[3] + parts.get(4).copied().unwrap_or(0);
        let total: u64 = parts.iter().sum();
        let current = CpuSnapshot { total, idle };

        let percent = if let Some(last) = &self.last_cpu {
            let delta_total = current.total.saturating_sub(last.total) as f32;
            let delta_idle = current.idle.saturating_sub(last.idle) as f32;
            if delta_total <= 0.0 {
                0.0
            } else {
                ((delta_total - delta_idle) / delta_total) * 100.0
            }
        } else {
            0.0
        };

        self.last_cpu = Some(current);
        percent.clamp(0.0, 100.0)
    }

    fn read_mem_percent(&self) -> f32 {
        let content = match fs::read_to_string("/proc/meminfo") {
            Ok(v) => v,
            Err(_) => return 0.0,
        };

        let mut total_kib = 0.0;
        let mut avail_kib = 0.0;

        for line in content.lines() {
            if let Some(value) = line.strip_prefix("MemTotal:") {
                total_kib = first_number(value);
            } else if let Some(value) = line.strip_prefix("MemAvailable:") {
                avail_kib = first_number(value);
            }
        }

        if total_kib <= 0.0 {
            return 0.0;
        }

        ((total_kib - avail_kib) / total_kib * 100.0).clamp(0.0, 100.0)
    }

    fn read_volume_percent(&self) -> f32 {
        self.read_volume_via_pactl()
            .or_else(|| self.read_volume_via_wpctl())
            .or_else(|| self.read_volume_via_amixer())
            .unwrap_or(0.0)
    }

    fn is_sink_muted_pactl(&self) -> bool {
        let Ok(output) = Command::new("pactl")
            .args(["get-sink-mute", "@DEFAULT_SINK@"])
            .output()
        else {
            return false;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        text.contains("yes")
    }

    fn read_volume_via_pactl(&self) -> Option<f32> {
        if self.is_sink_muted_pactl() {
            return Some(0.0);
        }
        let output = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        parse_percent_from_text(&String::from_utf8_lossy(&output.stdout))
    }

    fn read_volume_via_wpctl(&self) -> Option<f32> {
        let output = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        if text.contains("[MUTED]") {
            return Some(0.0);
        }
        for token in text.split_whitespace() {
            if let Ok(value) = token.parse::<f32>() {
                return Some((value * 100.0).clamp(0.0, 100.0));
            }
        }
        None
    }

    fn read_volume_via_amixer(&self) -> Option<f32> {
        let output = Command::new("amixer").arg("get").arg("Master").output().ok()?;
        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        if text.contains("[off]") {
            return Some(0.0);
        }
        parse_percent_from_text(&text)
    }

    fn read_network_speed(&mut self, preferred_iface: Option<&str>) -> (f64, f64) {
        let content = match fs::read_to_string("/proc/net/dev") {
            Ok(v) => v,
            Err(_) => return (0.0, 0.0),
        };

        let mut chosen: Option<(String, u64, u64)> = None;

        for line in content.lines().skip(2) {
            let Some((iface_raw, stats_raw)) = line.split_once(':') else {
                continue;
            };
            let iface = iface_raw.trim().to_string();
            if iface == "lo" {
                continue;
            }

            let stats: Vec<u64> = stats_raw
                .split_whitespace()
                .filter_map(|v| v.parse::<u64>().ok())
                .collect();
            if stats.len() < 16 {
                continue;
            }

            let rx = stats[0];
            let tx = stats[8];

            if let Some(preferred) = preferred_iface {
                if iface == preferred {
                    chosen = Some((iface, rx, tx));
                    break;
                }
            } else if chosen.is_none() {
                chosen = Some((iface, rx, tx));
            }
        }

        let Some((iface, rx, tx)) = chosen else {
            return (0.0, 0.0);
        };

        let now = Instant::now();
        let (down_bps, up_bps) = if let Some(last) = &self.last_net {
            if last.iface == iface {
                if let Some(last_time) = last.at {
                    let dt = now.duration_since(last_time).as_secs_f64();
                    if dt > 0.0 {
                        (
                            rx.saturating_sub(last.rx) as f64 / dt,
                            tx.saturating_sub(last.tx) as f64 / dt,
                        )
                    } else {
                        (0.0, 0.0)
                    }
                } else {
                    (0.0, 0.0)
                }
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };

        self.last_net = Some(NetSnapshot {
            iface,
            rx,
            tx,
            at: Some(now),
        });

        (down_bps, up_bps)
    }

    fn read_keyboard_leds(&self) -> (bool, bool, bool) {
        let mut caps = false;
        let mut num = false;
        let mut scroll = false;

        let entries = match fs::read_dir("/sys/class/leds") {
            Ok(v) => v,
            Err(_) => return (false, false, false),
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let brightness = fs::read_to_string(entry.path().join("brightness"))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            let on = brightness > 0;

            if name.contains("::capslock") {
                caps = on;
            } else if name.contains("::numlock") {
                num = on;
            } else if name.contains("::scrolllock") {
                scroll = on;
            }
        }

        (caps, num, scroll)
    }
}

fn first_number(input: &str) -> f32 {
    input
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0)
}

fn parse_percent_from_text(input: &str) -> Option<f32> {
    for token in input.split_whitespace() {
        if token.starts_with('[') && token.ends_with("%]") {
            let value = token.trim_start_matches('[').trim_end_matches("%]");
            if let Ok(parsed) = value.parse::<f32>() {
                return Some(parsed.clamp(0.0, 100.0));
            }
        }

        if let Some(value) = token.strip_suffix('%') {
            let value = value.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
            if let Ok(parsed) = value.parse::<f32>() {
                return Some(parsed.clamp(0.0, 100.0));
            }
        }
    }

    None
}
