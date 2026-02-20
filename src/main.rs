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
use metrics::MetricsCollector;

const APEX5_VID: u16 = 0x1038;
const APEX5_PID: u16 = 0x161C;
const APEX5_INTERFACE: &str = "mi_01";

fn main() -> Result<()> {
    let opts = parse_options();
    let config = DashboardConfig::load(&opts.config_path)
        .with_context(|| format!("failed to load config from {}", opts.config_path.display()))?;

    let refresh_ms = config.refresh_rate_ms.max(33) as u64;
    let tick = Duration::from_millis(refresh_ms);

    let mut metrics = MetricsCollector::new();
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

    loop {
        let started = Instant::now();
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

        let elapsed = started.elapsed();
        if elapsed < tick {
            thread::sleep(tick - elapsed);
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
