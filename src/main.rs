mod canvas;
mod config;
mod dashboard;
mod hidraw;
mod metrics;

use std::env;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use config::DashboardConfig;
use dashboard::DashboardRenderer;
use hidraw::HidSender;
use metrics::{MetricIntervals, MetricsCollector};

const APEX5_VID: u16 = 0x1038;
const APEX5_PID: u16 = 0x161C;
const APEX5_INTERFACE: &str = "mi_01";

fn main() -> Result<()> {
    let opts = parse_options();
    let config = DashboardConfig::load(&opts.config_path)
        .with_context(|| format!("failed to load config from {}", opts.config_path.display()))?;

    let refresh_ms = config.refresh_rate_ms.max(16) as u64;
    let tick = Duration::from_millis(refresh_ms);

    let mut metrics = MetricsCollector::with_intervals(MetricIntervals {
        cpu_ms: config.widget_refresh_rate_ms("cpu").unwrap_or(refresh_ms as u32),
        memory_ms: config
            .widget_refresh_rate_ms("memory")
            .unwrap_or(refresh_ms as u32),
        volume_ms: config.widget_refresh_rate_ms("volume").unwrap_or(100),
        audio_ms: config
            .widget_refresh_rate_ms("volume")
            .unwrap_or(refresh_ms as u32)
            .clamp(12, 40),
        network_ms: config.widget_refresh_rate_ms("network").unwrap_or(1000),
        keyboard_ms: config.widget_refresh_rate_ms("keyboard").unwrap_or(50),
    });
    let mut renderer = DashboardRenderer::new(config.display.width, config.display.height);
    let mut sender = HidSender::new(APEX5_VID, APEX5_PID, APEX5_INTERFACE.to_string());

    eprintln!(
        "Running {} from {} at {}ms/frame",
        if config.config_name.is_empty() {
            "Dashboard"
        } else {
            &config.config_name
        },
        opts.config_path.display(),
        refresh_ms
    );

    let network_iface = config.preferred_network_interface();
    let mut next_tick = Instant::now();

    loop {
        if let Err(err) = run_once(
            &config,
            &network_iface,
            &mut metrics,
            &mut renderer,
            &mut sender,
        ) {
            eprintln!("send failed: {err}");
        }

        if opts.one {
            break;
        }

        next_tick += tick;
        let now = Instant::now();
        if now < next_tick {
            thread::sleep(next_tick - now);
        } else if now.duration_since(next_tick) > tick {
            next_tick = now;
        }
    }

    Ok(())
}

fn run_once(
    config: &DashboardConfig,
    network_iface: &Option<String>,
    metrics: &mut MetricsCollector,
    renderer: &mut DashboardRenderer,
    sender: &mut HidSender,
) -> Result<()> {
    let sample = metrics.sample(network_iface.as_deref());
    let frame = renderer.render(config, &sample);
    sender.send_frame(&frame)
}

struct Options {
    config_path: std::path::PathBuf,
    one: bool,
}

fn parse_options() -> Options {
    let mut config_path: Option<std::path::PathBuf> = None;
    let mut one = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if let Some(path) = args.next() {
                config_path = Some(path.into());
            }
        } else if arg == "--one" {
            one = true;
        }
    }

    let config_path = if let Some(path) = config_path {
        path
    } else {
        // Priority: local profiles/ → ~/.config/stained-steel/ → Go fallback
        let root_profile = std::path::PathBuf::from("profiles/dashboard.json");
        if root_profile.exists() {
            root_profile
        } else if let Some(home) = env::var_os("HOME") {
            let xdg = std::path::PathBuf::from(home).join(".config/stained-steel/dashboard.json");
            if xdg.exists() {
                xdg
            } else {
                std::path::PathBuf::from("Go/profiles/dashboard.json")
            }
        } else {
            std::path::PathBuf::from("Go/profiles/dashboard.json")
        }
    };

    Options { config_path, one }
}
