use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DashboardConfig {
    #[serde(default)]
    pub config_name: String,
    #[serde(default = "default_refresh_rate")]
    pub refresh_rate_ms: u32,
    pub display: Display,
    #[serde(default)]
    pub widgets: Vec<Widget>,
}

#[derive(Debug, Deserialize)]
pub struct Display {
    pub width: usize,
    pub height: usize,
    #[serde(default)]
    pub background: u8,
}

#[derive(Debug, Deserialize)]
pub struct Widget {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub refresh_rate_ms: Option<u32>,
    pub position: Position,
    #[serde(default)]
    pub interface: Option<String>,
    #[serde(default)]
    pub show_icon: bool,
    #[serde(default)]
    pub bar: Option<BarConfig>,
    #[serde(default)]
    pub graph: Option<GraphConfig>,
}

#[derive(Debug, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Deserialize)]
pub struct BarConfig {
    #[serde(default = "default_direction")]
    pub direction: String,
    #[serde(default)]
    pub border: bool,
}

#[derive(Debug, Deserialize)]
pub struct GraphConfig {
    #[serde(default)]
    pub history: usize,
}

impl DashboardConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)?;
        let cfg: DashboardConfig = serde_json::from_str(&raw)?;
        Ok(cfg)
    }

    pub fn preferred_network_interface(&self) -> Option<String> {
        self.widgets
            .iter()
            .find(|w| w.enabled && w.kind == "network")
            .and_then(|w| w.interface.clone())
    }

    pub fn widget_refresh_rate_ms(&self, kind: &str) -> Option<u32> {
        self.widgets
            .iter()
            .filter(|w| w.enabled && w.kind == kind)
            .filter_map(|w| w.refresh_rate_ms)
            .min()
    }
}

fn default_refresh_rate() -> u32 {
    33
}

fn default_enabled() -> bool {
    true
}

fn default_direction() -> String {
    "horizontal".to_string()
}
