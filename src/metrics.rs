use std::fs;
use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct MetricIntervals {
    pub cpu_ms: u32,
    pub memory_ms: u32,
    pub volume_ms: u32,
    pub audio_ms: u32,
    pub network_ms: u32,
    pub keyboard_ms: u32,
}

impl Default for MetricIntervals {
    fn default() -> Self {
        Self {
            cpu_ms: 0,
            memory_ms: 0,
            volume_ms: 100,
            audio_ms: 25,
            network_ms: 1000,
            keyboard_ms: 50,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetricsSample {
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub volume_percent: f32,
    pub audio_level: f32,
    pub audio_waveform: Vec<f32>,
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

struct AudioMonitorCapture {
    sink_name: String,
    child: Child,
}

pub struct MetricsCollector {
    intervals: MetricIntervals,
    last_cpu_percent: Option<(f32, Instant)>,
    last_mem_percent: Option<(f32, Instant)>,
    last_cpu: Option<CpuSnapshot>,
    last_net: Option<NetSnapshot>,
    last_network_speed: Option<((f64, f64), Instant)>,
    last_volume: Option<(f32, Instant)>,
    last_audio_level: Option<(f32, Instant)>,
    audio_level_ema: f32,
    audio_monitor: Option<AudioMonitorCapture>,
    cached_default_sink: Option<String>,
    cached_monitor_source: Option<String>,
    last_audio_route_probe: Option<Instant>,
    audio_fresh_buf: Vec<u8>,
    audio_scratch_buf: [u8; 512],
    last_keyboard_leds: Option<((bool, bool, bool), Instant)>,
    caps_led_path: Option<PathBuf>,
    num_led_path: Option<PathBuf>,
    scroll_led_path: Option<PathBuf>,
    led_paths_resolved: bool,
    last_audio_waveform: Vec<f32>,
}

impl MetricsCollector {
    pub fn with_intervals(intervals: MetricIntervals) -> Self {
        Self {
            intervals,
            last_cpu_percent: None,
            last_mem_percent: None,
            last_cpu: None,
            last_net: None,
            last_network_speed: None,
            last_volume: None,
            last_audio_level: None,
            audio_level_ema: 0.0,
            audio_monitor: None,
            cached_default_sink: None,
            cached_monitor_source: None,
            last_audio_route_probe: None,
            audio_fresh_buf: Vec::with_capacity(1024),
            audio_scratch_buf: [0u8; 512],
            last_keyboard_leds: None,
            caps_led_path: None,
            num_led_path: None,
            scroll_led_path: None,
            led_paths_resolved: false,
            last_audio_waveform: Vec::with_capacity(128),
        }
    }

    pub fn sample(&mut self, preferred_iface: Option<&str>) -> MetricsSample {
        let cpu_percent = self.read_cpu_percent();
        let mem_percent = self.read_mem_percent();
        let volume_percent = self.read_volume_percent();
        let audio_level = self.read_audio_output_level();
        let (net_down_bps, net_up_bps) = self.read_network_speed(preferred_iface);
        let (caps_lock, num_lock, scroll_lock) = self.read_keyboard_leds();

        MetricsSample {
            cpu_percent,
            mem_percent,
            volume_percent,
            audio_level,
            audio_waveform: self.last_audio_waveform.clone(),
            net_up_bps,
            net_down_bps,
            caps_lock,
            num_lock,
            scroll_lock,
        }
    }

    fn read_audio_output_level(&mut self) -> f32 {
        let interval = Duration::from_millis(self.intervals.audio_ms as u64);
        if let Some((cached, at)) = self.last_audio_level
            && interval.as_millis() > 0
            && at.elapsed() < interval
        {
            return cached;
        }

        let raw = self.read_output_monitor_level().unwrap_or(0.0);
        let noise_floor = 1.4f32;
        let trimmed = (raw - noise_floor).max(0.0);

        self.audio_level_ema = self.audio_level_ema * 0.80 + trimmed * 0.20;
        let filtered = if self.audio_level_ema < 0.7 {
            0.0
        } else {
            self.audio_level_ema
        }
        .clamp(0.0, 100.0);

        self.last_audio_level = Some((filtered, Instant::now()));
        filtered
    }

    fn read_cpu_percent(&mut self) -> f32 {
        let interval = Duration::from_millis(self.intervals.cpu_ms as u64);
        if let Some((cached, at)) = self.last_cpu_percent
            && interval.as_millis() > 0
            && at.elapsed() < interval
        {
            return cached;
        }

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
        let value = percent.clamp(0.0, 100.0);
        self.last_cpu_percent = Some((value, Instant::now()));
        value
    }

    fn read_mem_percent(&mut self) -> f32 {
        let interval = Duration::from_millis(self.intervals.memory_ms as u64);
        if let Some((cached, at)) = self.last_mem_percent
            && interval.as_millis() > 0
            && at.elapsed() < interval
        {
            return cached;
        }

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

        let value = ((total_kib - avail_kib) / total_kib * 100.0).clamp(0.0, 100.0);
        self.last_mem_percent = Some((value, Instant::now()));
        value
    }

    fn read_volume_percent(&mut self) -> f32 {
        let volume_sample_interval = Duration::from_millis(self.intervals.volume_ms as u64);

        if let Some((cached, at)) = self.last_volume
            && volume_sample_interval.as_millis() > 0
            && at.elapsed() < volume_sample_interval
        {
            return cached;
        }

        let volume = self
            .read_volume_via_wpctl()
            .or_else(|| self.read_volume_via_pactl())
            .or_else(|| self.read_volume_via_amixer())
            .unwrap_or(0.0);

        self.last_volume = Some((volume, Instant::now()));
        volume
    }

    fn read_volume_via_pactl(&self) -> Option<f32> {
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

    fn default_sink_name_pactl(&self) -> Option<String> {
        let output = Command::new("pactl").arg("get-default-sink").output().ok()?;
        if output.status.success() {
            let sink = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !sink.is_empty() {
                return Some(sink);
            }
        }

        let output = Command::new("pactl").arg("info").output().ok()?;
        if !output.status.success() {
            return None;
        }

        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(rest) = line.trim().strip_prefix("Default Sink:") {
                let sink = rest.trim().to_string();
                if !sink.is_empty() {
                    return Some(sink);
                }
            }
        }
        None
    }

    fn default_sink_monitor_source_pactl(&self) -> Option<String> {
        let sink_name = self.default_sink_name_pactl()?;
        let fallback = format!("{sink_name}.monitor");

        let short_sources = Command::new("pactl")
            .args(["list", "short", "sources"])
            .output()
            .ok();
        let source_exists = |name: &str| -> bool {
            short_sources
                .as_ref()
                .filter(|out| out.status.success())
                .map(|out| {
                    String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .any(|line| line.split_whitespace().nth(1) == Some(name))
                })
                .unwrap_or(false)
        };

        let output = Command::new("pactl")
            .args(["list", "sinks"])
            .output()
            .ok()?;
        if output.status.success() {
            for block in String::from_utf8_lossy(&output.stdout).split("Sink #") {
                let mut name: Option<String> = None;
                let mut monitor_source: Option<String> = None;

                for line in block.lines() {
                    let trimmed = line.trim();
                    if let Some(v) = trimmed.strip_prefix("Name:") {
                        name = Some(v.trim().to_string());
                    } else if let Some(v) = trimmed.strip_prefix("Monitor Source:") {
                        monitor_source = Some(v.trim().to_string());
                    }
                }

                if let (Some(n), Some(m)) = (name, monitor_source)
                    && n == sink_name
                    && m.ends_with(".monitor")
                    && source_exists(&m)
                {
                    return Some(m);
                }
            }
        }

        if source_exists(&fallback) {
            Some(fallback)
        } else {
            None
        }
    }

    fn stop_audio_monitor(&mut self) {
        if let Some(mut capture) = self.audio_monitor.take() {
            let _ = capture.child.kill();
            let _ = capture.child.wait();
        }
    }

    fn refresh_audio_route_if_needed(&mut self, force: bool) {
        let should_probe = force
            || self.cached_default_sink.is_none()
            || self.cached_monitor_source.is_none()
            || self
                .last_audio_route_probe
                .is_none_or(|at| at.elapsed() >= Duration::from_millis(1500));

        if !should_probe {
            return;
        }

        self.last_audio_route_probe = Some(Instant::now());
        if let Some(sink) = self.default_sink_name_pactl() {
            self.cached_default_sink = Some(sink);
        }
        if let Some(mon) = self.default_sink_monitor_source_pactl() {
            self.cached_monitor_source = Some(mon);
        }
    }

    fn ensure_audio_monitor(&mut self) -> Option<()> {
        self.refresh_audio_route_if_needed(false);
        let sink_name = self.cached_default_sink.clone()?;

        if let Some(existing) = &self.audio_monitor
            && existing.sink_name == sink_name
        {
            return Some(());
        }

        self.stop_audio_monitor();
        self.refresh_audio_route_if_needed(true);

        let monitor_name = self.cached_monitor_source.clone()?;
        let mut child = Command::new("parec")
            .args([
                "-d",
                &monitor_name,
                "--raw",
                "--format=s16le",
                "--rate=8000",
                "--channels=1",
                "--latency-msec=20",
                "--process-time-msec=20",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        if !Self::set_child_stdout_nonblocking(&mut child) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }

        self.audio_monitor = Some(AudioMonitorCapture { sink_name, child });
        Some(())
    }

    fn set_child_stdout_nonblocking(child: &mut Child) -> bool {
        let Some(stdout) = child.stdout.as_ref() else {
            return false;
        };
        let fd = stdout.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 {
                return false;
            }
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == 0
        }
    }

    fn read_output_monitor_level(&mut self) -> Option<f32> {
        self.ensure_audio_monitor()?;

        const SAMPLE_COUNT: usize = 128;
        let target_bytes = SAMPLE_COUNT * 2;

        let Some(capture) = self.audio_monitor.as_mut() else {
            self.stop_audio_monitor();
            return None;
        };

        if let Ok(Some(_)) = capture.child.try_wait() {
            self.stop_audio_monitor();
            return None;
        }

        let Some(stdout) = capture.child.stdout.as_mut() else {
            self.stop_audio_monitor();
            return None;
        };

        // Limit iterations to avoid CPU spin when lots of data available
        for _ in 0..4 {
            match stdout.read(&mut self.audio_scratch_buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.audio_fresh_buf.extend_from_slice(&self.audio_scratch_buf[..n]);
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.stop_audio_monitor();
                    return None;
                }
            }
        }

        // Keep only the tail we need (more efficient than drain)
        if self.audio_fresh_buf.len() > 4096 {
            let keep_start = self.audio_fresh_buf.len() - 4096;
            self.audio_fresh_buf.copy_within(keep_start.., 0);
            self.audio_fresh_buf.truncate(4096);
        }

        if self.audio_fresh_buf.len() < 24 {
            self.last_audio_waveform.clear();
            return Some(0.0);
        }

        let start = self.audio_fresh_buf.len().saturating_sub(target_bytes);
        let bytes = &self.audio_fresh_buf[start
            ..self.audio_fresh_buf.len() - (self.audio_fresh_buf.len() - start) % 2];

        self.last_audio_waveform.clear();
        let mut sum_sq = 0.0f64;
        let mut n = 0usize;
        for chunk in bytes.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
            self.last_audio_waveform.push(sample);
            sum_sq += (sample as f64) * (sample as f64);
            n += 1;
        }
        if n == 0 {
            return Some(0.0);
        }

        let rms = (sum_sq / n as f64).sqrt() as f32;
        if rms < 0.0008 {
            self.last_audio_waveform.clear();
            return Some(0.0);
        }

        let normalized = ((rms - 0.0008) / 0.018).clamp(0.0, 1.0);
        Some(normalized * 100.0)
    }

    fn read_network_speed(&mut self, preferred_iface: Option<&str>) -> (f64, f64) {
        let network_sample_interval = Duration::from_millis(self.intervals.network_ms as u64);

        if let Some((cached, at)) = self.last_network_speed
            && network_sample_interval.as_millis() > 0
            && at.elapsed() < network_sample_interval
        {
            return cached;
        }

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

        let speeds = (down_bps, up_bps);
        self.last_network_speed = Some((speeds, Instant::now()));
        speeds
    }

    fn read_keyboard_leds(&mut self) -> (bool, bool, bool) {
        let led_sample_interval = Duration::from_millis(self.intervals.keyboard_ms as u64);

        if let Some((cached, at)) = self.last_keyboard_leds
            && led_sample_interval.as_millis() > 0
            && at.elapsed() < led_sample_interval
        {
            return cached;
        }

        if !self.led_paths_resolved {
            self.resolve_keyboard_led_paths();
        }

        let caps = self
            .caps_led_path
            .as_ref()
            .map(|p| Self::read_led_brightness_bool(p))
            .unwrap_or(false);
        let num = self
            .num_led_path
            .as_ref()
            .map(|p| Self::read_led_brightness_bool(p))
            .unwrap_or(false);
        let scroll = self
            .scroll_led_path
            .as_ref()
            .map(|p| Self::read_led_brightness_bool(p))
            .unwrap_or(false);

        let leds = (caps, num, scroll);
        self.last_keyboard_leds = Some((leds, Instant::now()));
        leds
    }

    fn resolve_keyboard_led_paths(&mut self) {
        self.led_paths_resolved = true;

        let entries = match fs::read_dir("/sys/class/leds") {
            Ok(v) => v,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let brightness = entry.path().join("brightness");
            if name.contains("::capslock") {
                self.caps_led_path = Some(brightness);
            } else if name.contains("::numlock") {
                self.num_led_path = Some(brightness);
            } else if name.contains("::scrolllock") {
                self.scroll_led_path = Some(brightness);
            }
        }
    }

    fn read_led_brightness_bool(path: &PathBuf) -> bool {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
            > 0
    }
}

impl Drop for MetricsCollector {
    fn drop(&mut self) {
        self.stop_audio_monitor();
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
