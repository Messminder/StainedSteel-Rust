use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::time::{Duration, Instant};
use crate::canvas::Canvas;
use crate::config::{DashboardConfig, Position, Widget};
use crate::metrics::MetricsSample;
use crate::weather::{WeatherCache, WeatherCondition};

#[derive(Clone, Copy)]
#[allow(dead_code)] // Keep other transitions available for future use
enum TransitionType {
    Noise,
    Spiral,
    Radial,
    Diamond,
    DoomMelt,
    Scanlines,
    Blinds,
}

pub struct DashboardRenderer {
    canvas: Canvas,
    width: usize,
    height: usize,
    boot_started: Instant,
    boot_duration: Duration,
    mem_history: VecDeque<f32>,
    volume_display: Option<i32>,
    volume_target: i32,
    vol_step_from: i32,
    vol_step_to: i32,
    vol_anim_step: u8,
    vol_anim_len: u8,
    vol_anim_speed: u8,
    prev_caps_lock: Option<bool>,
    caps_anim_step: u8,
    caps_anim_len: u8,
    caps_anim_from: bool,
    caps_anim_to: bool,
    prev_num_lock: Option<bool>,
    num_anim_step: u8,
    num_anim_len: u8,
    num_anim_from: bool,
    num_anim_to: bool,
    prev_scroll_lock: Option<bool>,
    scroll_anim_step: u8,
    scroll_anim_len: u8,
    scroll_anim_from: bool,
    scroll_anim_to: bool,
    silence_start: Option<Instant>,
    idle_sine_phase: f32,
    sep_sine_phase: f32,
    // Clock / volume overlay state
    show_volume_overlay: bool,
    prev_volume_state: Option<(i32, bool)>,  // (volume_rounded, is_muted)
    volume_overlay_start: Option<Instant>,
    colon_blink: Instant,
    // Transition animation (0.0 = clock fully visible, 1.0 = volume fully visible)
    volume_transition: f32,
    volume_transition_target: f32,
    transition_type: TransitionType,
    melt_seed: u32, // Random seed for DOOM melt pattern
    // Weather
    weather: WeatherCache,
    weather_anim_phase: f32,
}

impl DashboardRenderer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            canvas: Canvas::new(width, height),
            width,
            height,
            boot_started: Instant::now(),
            boot_duration: Duration::from_millis(2100),
            mem_history: VecDeque::new(),
            volume_display: None,
            volume_target: 0,
            vol_step_from: 0,
            vol_step_to: 0,
            vol_anim_step: 0,
            vol_anim_len: 10,
            vol_anim_speed: 1,
            prev_caps_lock: None,
            caps_anim_step: 0,
            caps_anim_len: 6,
            caps_anim_from: false,
            caps_anim_to: false,
            prev_num_lock: None,
            num_anim_step: 0,
            num_anim_len: 6,
            num_anim_from: false,
            num_anim_to: false,
            prev_scroll_lock: None,
            scroll_anim_step: 0,
            scroll_anim_len: 6,
            scroll_anim_from: false,
            scroll_anim_to: false,
            silence_start: None,
            idle_sine_phase: 0.0,
            sep_sine_phase: 0.0,
            show_volume_overlay: false,
            prev_volume_state: None,
            volume_overlay_start: None,
            colon_blink: Instant::now(),
            volume_transition: 0.0,
            volume_transition_target: 0.0,
            transition_type: TransitionType::DoomMelt,
            melt_seed: {
                let mut tv = libc::timespec { tv_sec: 0, tv_nsec: 0 };
                unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut tv); }
                tv.tv_nsec as u32
            },
            weather: WeatherCache::new(),
            weather_anim_phase: 0.0,
        }
    }

    fn pick_random_transition(&mut self) {
        // DOOM-style melt is so good, it's the only one we need
        self.transition_type = TransitionType::DoomMelt;
        // New random seed each transition based on wall clock time
        let mut tv = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut tv); }
        self.melt_seed = (tv.tv_sec as u32).wrapping_mul(1000000000).wrapping_add(tv.tv_nsec as u32);
    }

    pub fn render(&mut self, config: &DashboardConfig, sample: &MetricsSample) -> Vec<u8> {
        self.canvas.clear(config.display.background > 0);

        let elapsed = self.boot_started.elapsed();
        if elapsed < self.boot_duration {
            let progress = (elapsed.as_secs_f32() / self.boot_duration.as_secs_f32()).clamp(0.0, 1.0);
            self.draw_boot_logo(progress);
            return self.canvas.to_packed_bytes();
        }

        for widget in &config.widgets {
            if !widget.enabled {
                continue;
            }

            match widget.kind.as_str() {
                "cpu" => self.draw_cpu(widget, sample),
                "volume" => {
                    self.update_volume_overlay(sample);
                    self.draw_volume_clock_transition(widget, sample);
                }
                "memory" => self.draw_memory(widget, sample),
                "network" => self.draw_network(widget, sample),
                "keyboard" => self.draw_keyboard(widget, sample),
                _ => {}
            }
        }

        self.draw_sine_wave_gap(config, sample);
        self.draw_mem_net_separator(config);

        self.canvas.to_packed_bytes()
    }

    fn draw_sine_wave_gap(&mut self, config: &DashboardConfig, sample: &MetricsSample) {
        let volume = match config.widgets.iter().find(|w| w.enabled && w.kind == "volume") {
            Some(w) => w,
            None => return,
        };
        let keyboard = match config.widgets.iter().find(|w| w.enabled && w.kind == "keyboard") {
            Some(w) => w,
            None => return,
        };
        let network = match config.widgets.iter().find(|w| w.enabled && w.kind == "network") {
            Some(w) => w,
            None => return,
        };

        let keyboard_bottom = keyboard.position.y + keyboard.position.h - 1;
        let band_top = keyboard_bottom + 1;
        let band_bottom = network.position.y - 1;

        let volume_right = volume.position.x + volume.position.w - 1;
        let clip_left = keyboard
            .position
            .x
            .max(network.position.x)
            .max(volume_right + 1);
        let clip_right = (keyboard.position.x + keyboard.position.w - 1)
            .min(network.position.x + network.position.w - 1);

        let draw_top = band_top;
        let draw_bottom = band_bottom;
        if draw_top > draw_bottom || clip_left >= clip_right {
            return;
        }

        let center_y = (draw_top + draw_bottom) / 2;
        let max_amp = ((draw_bottom - draw_top) / 2).max(1) as f32;
        let level = (sample.audio_level / 100.0).clamp(0.0, 1.0);
        let silence_gate = 0.02f32;
        let is_silent = sample.volume_percent <= 0.0 || sample.audio_waveform.is_empty() || level <= silence_gate;

        // Track silence duration
        if is_silent {
            if self.silence_start.is_none() {
                self.silence_start = Some(Instant::now());
            }
        } else {
            self.silence_start = None;
        }

        // Calculate idle sine blend (0.0 = flatline, 1.0 = full sine)
        let idle_blend = if let Some(start) = self.silence_start {
            let elapsed = start.elapsed().as_secs_f32();
            if elapsed < 5.0 {
                0.0
            } else {
                ((elapsed - 5.0) / 2.0).clamp(0.0, 1.0) // Fade in over 2 seconds
            }
        } else {
            0.0
        };

        // Animate idle sine phase
        if idle_blend > 0.0 {
            self.idle_sine_phase = (self.idle_sine_phase + 0.15).rem_euclid(TAU);
        }

        let width = (clip_right - clip_left + 1) as usize;

        if is_silent && idle_blend <= 0.0 {
            // Pure flatline
            self.canvas.line(clip_left, center_y, clip_right, center_y, true);
            return;
        }

        let waveform = &sample.audio_waveform;
        let waveform_len = waveform.len();
        let gain = 5.0f32;
        let wavelength = 20.0f32;

        let mut prev_y: Option<i32> = None;
        for (i, x) in (clip_left..=clip_right).enumerate() {
            let y = if is_silent {
                // Idle sine wave
                let theta = ((x - clip_left) as f32 / wavelength) * TAU + self.idle_sine_phase;
                let sine_val = theta.sin() * idle_blend * max_amp * 0.6;
                (center_y as f32 - sine_val).round().clamp(draw_top as f32, draw_bottom as f32) as i32
            } else {
                // Real waveform
                let sample_idx = (i * waveform_len) / width.max(1);
                let sample_val = waveform.get(sample_idx).copied().unwrap_or(0.0);
                (center_y as f32 - sample_val * gain * max_amp)
                    .round()
                    .clamp(draw_top as f32, draw_bottom as f32) as i32
            };

            if let Some(py) = prev_y {
                self.canvas.line(x - 1, py, x, y, true);
            } else {
                self.canvas.set(x, y, true);
            }
            prev_y = Some(y);
        }
    }

    fn draw_mem_net_separator(&mut self, config: &DashboardConfig) {
        let memory = match config.widgets.iter().find(|w| w.enabled && w.kind == "memory") {
            Some(w) => w,
            None => return,
        };
        let network = match config.widgets.iter().find(|w| w.enabled && w.kind == "network") {
            Some(w) => w,
            None => return,
        };

        let gap_left = memory.position.x + memory.position.w;
        let gap_right = network.position.x - 1;
        if gap_left > gap_right {
            return;
        }

        let top = memory.position.y.max(network.position.y) - 2;
        let bottom = (memory.position.y + memory.position.h - 1)
            .min(network.position.y + network.position.h - 1);
        if top > bottom {
            return;
        }

        let center_x = (gap_left + gap_right) / 2 + 1;
        let max_amp = ((gap_right - gap_left) / 2).max(1) as f32;
        let wavelength = 12.0f32;

        self.sep_sine_phase = (self.sep_sine_phase + 0.10).rem_euclid(TAU);

        let mut prev_x: Option<i32> = None;
        let mut prev_y_coord: Option<i32> = None;
        for (i, y) in (top..=bottom).enumerate() {
            let theta = (i as f32 / wavelength) * TAU + self.sep_sine_phase;
            let x = (center_x as f32 + theta.sin() * max_amp * 0.7)
                .round()
                .clamp(gap_left as f32, gap_right as f32) as i32;

            if let (Some(px), Some(py)) = (prev_x, prev_y_coord) {
                self.canvas.line(px, py, x, y, true);
            } else {
                self.canvas.set(x, y, true);
            }
            prev_x = Some(x);
            prev_y_coord = Some(y);
        }
    }

    fn draw_boot_logo(&mut self, progress: f32) {
        let cx = (self.width as i32) / 2;
        let cy = (self.height as i32) / 2 - 2;

        // Final section dissolves everything out before handoff.
        let dissolve_start = 0.84;
        let dissolve_t = if progress > dissolve_start {
            ((progress - dissolve_start) / (1.0 - dissolve_start)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let keep = |seed: usize| -> bool {
            if dissolve_t <= 0.0 {
                return true;
            }
            let noise = ((seed * 37 + 17) % 100) as f32 / 100.0;
            noise > dissolve_t
        };

        let teeth = 12usize;
        let reveal = ((teeth as f32) * progress).ceil() as usize;
        let rotation = TAU * 1.75 * progress;

        // Inner dotted ring
        for i in 0..24 {
            if !keep(100 + i) {
                continue;
            }
            let a = (i as f32 / 24.0) * TAU - rotation * 0.4;
            let drift = dissolve_t * 6.0;
            let x = cx + (a.cos() * (10.0 + drift)).round() as i32;
            let y = cy + (a.sin() * (10.0 + drift)).round() as i32;
            self.canvas.set(x, y, true);
        }

        // Tooth-by-tooth outer gear reveal + rotation
        for i in 0..reveal.min(teeth) {
            if !keep(200 + i) {
                continue;
            }
            let a = rotation + (i as f32 / teeth as f32) * TAU;
            let drift = dissolve_t * 8.0;
            let x0 = cx + (a.cos() * (13.0 + drift * 0.6)).round() as i32;
            let y0 = cy + (a.sin() * (13.0 + drift * 0.6)).round() as i32;
            let x1 = cx + (a.cos() * (18.0 + drift)).round() as i32;
            let y1 = cy + (a.sin() * (18.0 + drift)).round() as i32;

            self.canvas.line(x0, y0, x1, y1, true);

            // tiny tooth cap for chunkier gear feel
            let px = -(a.sin()).round() as i32;
            let py = (a.cos()).round() as i32;
            self.canvas.line(x1 - px, y1 - py, x1 + px, y1 + py, true);
        }

        // Center mark: 4-pointed star
        if keep(300) {
            let base_arm = if progress < 0.6 { 4 } else { 6 };

            // Pulse 3 times from animation start until dissolve begins.
            let pulse_t = (progress / dissolve_start).clamp(0.0, 1.0);
            let pulse_wave = (pulse_t * 3.0 * TAU).sin() * 0.5 + 0.5;
            let arm = base_arm + (pulse_wave * 2.0).round() as i32;

            // Core
            self.canvas.set(cx, cy, true);
            self.canvas.set(cx - 1, cy, true);
            self.canvas.set(cx + 1, cy, true);
            self.canvas.set(cx, cy - 1, true);
            self.canvas.set(cx, cy + 1, true);

            // Briefly thicken center at pulse peaks for stronger heartbeat effect.
            if pulse_wave > 0.85 {
                self.canvas.set(cx - 1, cy - 1, true);
                self.canvas.set(cx + 1, cy - 1, true);
                self.canvas.set(cx - 1, cy + 1, true);
                self.canvas.set(cx + 1, cy + 1, true);
            }

            // Four primary points
            self.canvas.line(cx, cy - 1, cx, cy - arm, true);
            self.canvas.line(cx, cy + 1, cx, cy + arm, true);
            self.canvas.line(cx - 1, cy, cx - arm, cy, true);
            self.canvas.line(cx + 1, cy, cx + arm, cy, true);

            // Slight inner taper for sparkle feel
            let taper = (arm / 2).max(2);
            for d in 1..=taper {
                self.canvas.set(cx - d, cy - d, true);
                self.canvas.set(cx + d, cy - d, true);
                self.canvas.set(cx - d, cy + d, true);
                self.canvas.set(cx + d, cy + d, true);
            }
        }

        // Finale phase: lock ring, burst, then ring dissipation.
        let finale_start = 0.78;
        if progress >= finale_start {
            let finale_t = ((progress - finale_start) / (1.0 - finale_start)).clamp(0.0, 1.0);

            // Lock-in ring then dissipate outward.
            let ring_dissipate = ((finale_t - 0.45) / 0.55).clamp(0.0, 1.0);
            for i in 0..40 {
                if !keep(400 + i) {
                    continue;
                }
                let a = (i as f32 / 40.0) * TAU;
                let r = 21.0 + ring_dissipate * 10.0;
                let x = cx + (a.cos() * r).round() as i32;
                let y = cy + (a.sin() * r).round() as i32;
                self.canvas.set(x, y, true);
            }

            // Spark burst expands then retracts
            let burst_t = if finale_t < 0.58 {
                finale_t / 0.58
            } else {
                1.0 - ((finale_t - 0.58) / 0.42)
            }
            .clamp(0.0, 1.0);
            let burst_len = (burst_t * 8.0).round() as i32;
            for i in 0..8 {
                let a = (i as f32 / 8.0) * TAU;
                let x0 = cx + (a.cos() * 21.0).round() as i32;
                let y0 = cy + (a.sin() * 21.0).round() as i32;
                let x1 = cx + (a.cos() * (21.0 + burst_len as f32)).round() as i32;
                let y1 = cy + (a.sin() * (21.0 + burst_len as f32)).round() as i32;
                self.canvas.line(x0, y0, x1, y1, true);
            }

            // Shine pass through center
            let shine_x = cx - 14 + (finale_t * 28.0).round() as i32;
            self.canvas.line(shine_x, cy - 8, shine_x, cy + 8, true);
        }
    }

    fn draw_cpu(&mut self, widget: &Widget, sample: &MetricsSample) {
        self.draw_bar(
            &widget.position,
            sample.cpu_percent,
            widget
                .bar
                .as_ref()
                .map(|b| b.direction.as_str())
                .unwrap_or("vertical"),
            widget.bar.as_ref().map(|b| b.border).unwrap_or(false),
        );
        self.draw_cpu_icon(&widget.position);
    }

    /// Draws a tiny CPU chip icon (8×9) at the top of the widget,
    /// 2px from top border, using invert for visibility.
    fn draw_cpu_icon(&mut self, pos: &Position) {
        // 8 wide × 9 tall chip icon
        #[rustfmt::skip]
        const CHIP: [[u8; 8]; 9] = [
            [0,0,1,0,0,1,0,0], // top pins
            [0,1,1,1,1,1,1,0], // top edge
            [0,1,0,0,0,0,1,0], // body
            [1,1,0,0,0,0,1,1], // side pins
            [0,1,0,1,1,0,1,0], // body + die mark
            [1,1,0,0,0,0,1,1], // side pins
            [0,1,0,0,0,0,1,0], // body
            [0,1,1,1,1,1,1,0], // bottom edge
            [0,0,1,0,0,1,0,0], // bottom pins
        ];

        let icon_w = 8_i32;
        let ox = pos.x + (pos.w - icon_w) / 2;
        let oy = pos.y + 2; // 2px from top border

        for (row, cols) in CHIP.iter().enumerate() {
            for (col, &px) in cols.iter().enumerate() {
                if px == 1 {
                    self.canvas.invert(ox + col as i32, oy + row as i32);
                }
            }
        }
    }

    fn update_volume_overlay(&mut self, sample: &MetricsSample) {
        let vol_now = sample.volume_percent.round() as i32;
        let muted_now = sample.is_muted;
        let state_now = (vol_now, muted_now);

        if let Some(prev) = self.prev_volume_state {
            if state_now != prev {
                // Volume or mute state changed → show overlay
                self.show_volume_overlay = true;
                self.volume_overlay_start = Some(Instant::now());
                self.volume_transition_target = 1.0; // transition to volume
                self.pick_random_transition();
            }
        }
        self.prev_volume_state = Some(state_now);

        // Auto-hide volume overlay after 3 seconds
        if self.show_volume_overlay {
            if let Some(start) = self.volume_overlay_start {
                if start.elapsed() > Duration::from_secs(3) {
                    self.show_volume_overlay = false;
                    self.volume_overlay_start = None;
                    self.volume_transition_target = 0.0; // transition back to clock
                    self.pick_random_transition();
                }
            }
        }

        // Smoothly interpolate transition progress toward target
        let speed = 0.15; // 15% per frame (smooth ease)
        if (self.volume_transition - self.volume_transition_target).abs() > 0.01 {
            self.volume_transition += (self.volume_transition_target - self.volume_transition) * speed;
        } else {
            self.volume_transition = self.volume_transition_target;
        }
    }

    fn draw_volume_clock_transition(&mut self, widget: &Widget, sample: &MetricsSample) {
        let p = &widget.position;
        let progress = self.volume_transition;

        // Optimization: skip blend when fully one or the other
        if progress <= 0.0 {
            self.draw_clock(widget);
            return;
        }
        if progress >= 1.0 {
            self.draw_volume(widget, sample);
            return;
        }

        // Draw both widgets to temp canvases
        let mut clock_canvas = Canvas::new(self.width, self.height);
        std::mem::swap(&mut self.canvas, &mut clock_canvas);
        self.draw_clock(widget);
        std::mem::swap(&mut self.canvas, &mut clock_canvas);

        let mut volume_canvas = Canvas::new(self.width, self.height);
        std::mem::swap(&mut self.canvas, &mut volume_canvas);
        self.draw_volume(widget, sample);
        std::mem::swap(&mut self.canvas, &mut volume_canvas);

        // Apply selected transition effect
        let frame_seed = (self.colon_blink.elapsed().as_millis() / 16) as u32;
        let cx = p.w as f32 / 2.0;
        let cy = p.h as f32 / 2.0;
        let max_dist = (cx * cx + cy * cy).sqrt();

        for y in p.y..(p.y + p.h) {
            for x in p.x..(p.x + p.w) {
                let lx = (x - p.x) as f32;
                let ly = (y - p.y) as f32;

                let use_volume = match self.transition_type {
                    TransitionType::Noise => {
                        // Random noise dissolve
                        let hash = ((x as u32).wrapping_mul(2654435761))
                            ^ ((y as u32).wrapping_mul(2246822519))
                            ^ frame_seed.wrapping_mul(374761393);
                        let threshold = (hash % 256) as f32 / 255.0;
                        progress > threshold
                    }
                    TransitionType::Spiral => {
                        // Spiral from center outward
                        let dx = lx - cx;
                        let dy = ly - cy;
                        let dist = (dx * dx + dy * dy).sqrt() / max_dist;
                        let angle = dy.atan2(dx) / TAU + 0.5; // 0..1
                        let threshold = (dist * 0.6 + angle * 0.4).fract();
                        progress > threshold
                    }
                    TransitionType::Radial => {
                        // Circular reveal from center
                        let dx = lx - cx;
                        let dy = ly - cy;
                        let threshold = (dx * dx + dy * dy).sqrt() / max_dist;
                        progress > threshold
                    }
                    TransitionType::Diamond => {
                        // Diamond shape expanding from center
                        let dx = (lx - cx).abs() / cx;
                        let dy = (ly - cy).abs() / cy;
                        let threshold = (dx + dy) / 2.0;
                        progress > threshold
                    }
                    TransitionType::DoomMelt => {
                        // DOOM-style screen melt - columns drip down at staggered rates
                        let col = (x - p.x) as u32;
                        // Mix column with seed for varied random pattern each transition
                        let mixed = col.wrapping_add(self.melt_seed);
                        let col_hash = mixed.wrapping_mul(2654435761)
                            .wrapping_add(mixed.rotate_left(13))
                            .wrapping_mul(374761393);
                        let delay = ((col_hash % 256) as f32 / 255.0) * 0.5; // 0 to 0.5 delay
                        // How far down has this column melted?
                        let melt_progress = ((progress - delay) / (1.0 - delay)).clamp(0.0, 1.0);
                        let melt_y = melt_progress * p.h as f32;
                        // Pixels above melt_y show new content, below show old
                        ly < melt_y
                    }
                    TransitionType::Scanlines => {
                        // Alternating horizontal lines reveal at different rates
                        let row = (y - p.y) as usize;
                        let offset = if row % 2 == 0 { 0.0 } else { 0.3 };
                        let threshold = offset + (1.0 - offset) * (x - p.x) as f32 / p.w as f32;
                        progress > threshold
                    }
                    TransitionType::Blinds => {
                        // Vertical blinds (columns reveal in groups)
                        let col = (x - p.x) as usize;
                        let blind_width = 8;
                        let blind_idx = col / blind_width;
                        let offset = (blind_idx % 3) as f32 * 0.15;
                        let threshold = offset + (1.0 - offset) * (y - p.y) as f32 / p.h as f32;
                        progress > threshold
                    }
                };

                let pixel = if use_volume {
                    volume_canvas.get(x, y)
                } else {
                    clock_canvas.get(x, y)
                };
                self.canvas.set(x, y, pixel);
            }
        }
    }

    fn draw_clock(&mut self, widget: &Widget) {
        let p = &widget.position;

        // Get current time
        let now = {
            let mut tv = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut tv); } //LOOOOOOL
            tv.tv_sec
        };
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&now, &mut tm); } //DRIVER UNUSABLE. RUST WAS A MISTAKE. AI WILL TURN YOUR PC INTO A TIMEBOMB.
        let hours = tm.tm_hour as u32;
        let minutes = tm.tm_min as u32;
        let seconds = tm.tm_sec as u32;

        let blink_elapsed_ms = self.colon_blink.elapsed().as_millis();
        let colon_on = (blink_elapsed_ms % 1000) < 500;

        // === Seconds progress bar along the bottom (2px tall) ===
        let bar_y = p.y + p.h - 2;
        let bar_inner_w = p.w - 2; // 1px margin each side
        let fill_w = ((seconds as f32 / 59.0) * bar_inner_w as f32).round() as i32;
        // Bar track (dim dots every 2px)
        for x in 0..bar_inner_w {
            if x % 2 == 0 {
                self.canvas.set(p.x + 1 + x, bar_y + 1, true);
            }
        }
        // Bar fill (solid)
        for x in 0..fill_w {
            self.canvas.set(p.x + 1 + x, bar_y, true);
            self.canvas.set(p.x + 1 + x, bar_y + 1, true);
        }

        // === Tick marks along the top edge (like a watchface) ===
        let tick_y = p.y + 1;
        for i in 0..=12 {
            let tx = p.x + 1 + (i * (bar_inner_w - 1)) / 12;
            let tall = i % 3 == 0; // quarter marks are taller
            self.canvas.set(tx, tick_y, true);
            if tall {
                self.canvas.set(tx, tick_y + 1, true);
            }
        }

        // === Large HH:MM (scale 2) right-biased, with small :SS tight to the right ===
        let scale = 2;
        let glyph_w_lg = 4 * scale; // 8px actual glyph width
        let digit_advance = glyph_w_lg + 1; // 9px between digit starts (1px gap)
        let colon_advance = 3 * scale + 1; // 7px for colon/space (narrower)
        let text_h_lg = 5 * scale; // 10px tall

        let colon_char = if colon_on { ':' } else { ' ' };

        // Total width: HH + colon + MM
        let hm_w = 2 * digit_advance + colon_advance + 2 * digit_advance - 1;

        // Small seconds
        let char_w_sm = 5; // scale 1 advance
        let text_h_sm = 5;
        let ss_str = format!("{:02}", seconds);
        let ss_w = ss_str.len() as i32 * char_w_sm;

        // Tight gap between HH:MM and :SS
        let gap = 2;
        let total_w = hm_w + gap + ss_w;
        // Right-biased: shift toward right edge, leave 2px margin
        let base_x = p.x + p.w - total_w - 2;
        let hm_y = p.y + 4;
        let ss_y = hm_y + text_h_lg - text_h_sm;

        // === Weather icon (left side, replaces 12h clock) ===
        // Update weather data periodically
        self.weather.update();
        self.weather_anim_phase += 0.1; // Advance animation
        if self.weather_anim_phase > TAU * 100.0 {
            self.weather_anim_phase = 0.0; // Prevent overflow
        }
        
        let icon_x = p.x + 5;
        let icon_y = p.y + 4;
        let icon_size = 14; // 14x14 pixel area for weather icon
        
        self.draw_weather_icon(icon_x, icon_y, icon_size);

        // Draw HH:MM character by character with tighter spacing (24h military format)
        let h_str = format!("{:02}", hours);
        let m_str = format!("{:02}", minutes);
        let mut cx = base_x;
        for ch in h_str.chars() {
            self.canvas.draw_text_scaled(cx, hm_y, &ch.to_string(), scale);
            cx += digit_advance;
        }
        self.canvas.draw_text_scaled(cx, hm_y, &colon_char.to_string(), scale);
        cx += colon_advance;
        for ch in m_str.chars() {
            self.canvas.draw_text_scaled(cx, hm_y, &ch.to_string(), scale);
            cx += digit_advance;
        }

        // Small colon before seconds (decorative)
        let colon_x = cx;
        self.canvas.set(colon_x, ss_y + 1, true);
        self.canvas.set(colon_x, ss_y + 3, true);

        self.canvas.draw_text_scaled(cx + gap, ss_y, &ss_str, 1);

        // === Scanning dot — traces along the widget perimeter once per minute ===
        let perimeter = 2 * (p.w - 1) + 2 * (p.h - 1);
        let dot_pos = ((seconds as f32 / 60.0) * perimeter as f32).round() as i32;
        let (dx, dy) = self.perimeter_point(p.x, p.y, p.w, p.h, dot_pos);
        // Draw a 2px inverted "cursor"
        self.canvas.invert(dx, dy);
        let (dx2, dy2) = self.perimeter_point(p.x, p.y, p.w, p.h, (dot_pos + 1) % perimeter);
        self.canvas.invert(dx2, dy2);
    }

    /// Map a linear offset along a rectangle's perimeter to (x, y).
    fn perimeter_point(&self, rx: i32, ry: i32, rw: i32, rh: i32, offset: i32) -> (i32, i32) {
        let top = rw - 1;
        let right = rh - 1;
        let bottom = rw - 1;
        let _left = rh - 1;
        if offset < top {
            (rx + offset, ry)
        } else if offset < top + right {
            (rx + rw - 1, ry + offset - top)
        } else if offset < top + right + bottom {
            (rx + rw - 1 - (offset - top - right), ry + rh - 1)
        } else {
            (rx, ry + rh - 1 - (offset - top - right - bottom))
        }
    }

    fn draw_weather_icon(&mut self, x: i32, y: i32, size: i32) {
        let phase = self.weather_anim_phase;
        let condition = self.weather.condition;

        match condition {
            WeatherCondition::Sunny => self.draw_sun_icon(x, y, size, phase),
            WeatherCondition::PartlyCloudy => self.draw_partly_cloudy_icon(x, y, size, phase),
            WeatherCondition::Cloudy => self.draw_cloud_icon(x, y, size, phase),
            WeatherCondition::Fog => self.draw_fog_icon(x, y, size, phase),
            WeatherCondition::Drizzle => self.draw_drizzle_icon(x, y, size, phase),
            WeatherCondition::Rain => self.draw_rain_icon(x, y, size, phase),
            WeatherCondition::Thunderstorm => self.draw_thunderstorm_icon(x, y, size, phase),
            WeatherCondition::Snow => self.draw_snow_icon(x, y, size, phase),
            WeatherCondition::Unknown => self.draw_unknown_icon(x, y, size, phase),
        }
    }

    fn draw_sun_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Hand-crafted pixel art sun with pulsing glow
        let cx = x + 7;
        let cy = y + 6;
        
        // Sun core - clean circle
        #[rustfmt::skip]
        const SUN_CORE: [[u8; 7]; 7] = [
            [0,0,1,1,1,0,0],
            [0,1,1,1,1,1,0],
            [1,1,1,1,1,1,1],
            [1,1,1,1,1,1,1],
            [1,1,1,1,1,1,1],
            [0,1,1,1,1,1,0],
            [0,0,1,1,1,0,0],
        ];
        
        for (row, cols) in SUN_CORE.iter().enumerate() {
            for (col, &px) in cols.iter().enumerate() {
                if px == 1 {
                    self.canvas.set(cx - 3 + col as i32, cy - 3 + row as i32, true);
                }
            }
        }
        
        // Animated rays - 8 rays with alternating lengths
        let ray_phase = phase * 2.0;
        let pulse = (ray_phase.sin() * 0.5 + 0.5) as i32; // 0 or 1
        
        // Cardinal rays (longer)
        let long_len = 3 + pulse;
        // Top ray
        for i in 0..long_len { self.canvas.set(cx, cy - 4 - i, true); }
        // Bottom ray
        for i in 0..long_len { self.canvas.set(cx, cy + 4 + i, true); }
        // Left ray
        for i in 0..long_len { self.canvas.set(cx - 4 - i, cy, true); }
        // Right ray
        for i in 0..long_len { self.canvas.set(cx + 4 + i, cy, true); }
        
        // Diagonal rays (shorter, offset phase)
        let short_len = 2 + (1 - pulse);
        let diag_offset = 3;
        // Top-left
        for i in 0..short_len { self.canvas.set(cx - diag_offset - i, cy - diag_offset - i, true); }
        // Top-right
        for i in 0..short_len { self.canvas.set(cx + diag_offset + i, cy - diag_offset - i, true); }
        // Bottom-left
        for i in 0..short_len { self.canvas.set(cx - diag_offset - i, cy + diag_offset + i, true); }
        // Bottom-right
        for i in 0..short_len { self.canvas.set(cx + diag_offset + i, cy + diag_offset + i, true); }
    }

    fn draw_cloud_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Hand-crafted fluffy cloud with drift + bob animation
        let bob = ((phase * 1.5).sin() * 2.0) as i32;
        let drift = ((phase * 0.8).cos() * 1.5) as i32;
        let cx = x + 7 + drift;
        let cy = y + 6 + bob;
        
        // Fluffy cloud sprite - carefully designed
        #[rustfmt::skip]
        const CLOUD: [[u8; 14]; 9] = [
            [0,0,0,0,0,1,1,1,0,0,0,0,0,0],
            [0,0,0,0,1,1,1,1,1,0,0,0,0,0],
            [0,0,1,1,1,1,1,1,1,1,1,0,0,0],
            [0,1,1,1,1,1,1,1,1,1,1,1,0,0],
            [1,1,1,1,1,1,1,1,1,1,1,1,1,0],
            [1,1,1,1,1,1,1,1,1,1,1,1,1,1],
            [0,1,1,1,1,1,1,1,1,1,1,1,1,0],
            [0,0,1,1,1,1,1,1,1,1,1,1,0,0],
            [0,0,0,0,1,1,1,1,1,1,0,0,0,0],
        ];
        
        for (row, cols) in CLOUD.iter().enumerate() {
            for (col, &px) in cols.iter().enumerate() {
                if px == 1 {
                    self.canvas.set(cx - 7 + col as i32, cy - 4 + row as i32, true);
                }
            }
        }
    }

    fn draw_rain_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Cloud with stylized raindrops
        let cx = x + 7;
        let cy = y + 3;
        
        // Compact rain cloud
        #[rustfmt::skip]
        const RAIN_CLOUD: [[u8; 12]; 5] = [
            [0,0,0,1,1,1,1,1,0,0,0,0],
            [0,0,1,1,1,1,1,1,1,0,0,0],
            [0,1,1,1,1,1,1,1,1,1,0,0],
            [1,1,1,1,1,1,1,1,1,1,1,0],
            [0,1,1,1,1,1,1,1,1,1,0,0],
        ];
        
        for (row, cols) in RAIN_CLOUD.iter().enumerate() {
            for (col, &px) in cols.iter().enumerate() {
                if px == 1 {
                    self.canvas.set(cx - 6 + col as i32, cy - 2 + row as i32, true);
                }
            }
        }
        
        // Animated raindrops - elongated teardrop shapes
        let drops = [
            (cx - 4, 0.0),
            (cx - 1, 0.3),
            (cx + 2, 0.6),
            (cx + 5, 0.15),
        ];
        
        for (drop_x, offset) in drops {
            let drop_phase = (phase * 2.5 + offset * TAU) % TAU;
            let progress = drop_phase / TAU;
            let drop_y = cy + 4 + (progress * 8.0) as i32;
            
            if drop_y < y + 13 {
                // Teardrop shape: | at top, : below
                self.canvas.set(drop_x, drop_y, true);
                self.canvas.set(drop_x, drop_y + 1, true);
                if drop_y < y + 11 {
                    self.canvas.set(drop_x, drop_y + 2, true);
                }
            }
        }
    }

    fn draw_snow_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Elegant snowflakes falling gently
        // Each snowflake is a tiny * pattern
        
        let flakes = [
            (x + 2, 0.0, 0),
            (x + 6, 0.4, 1),
            (x + 10, 0.8, 0),
            (x + 4, 0.2, 1),
            (x + 8, 0.6, 0),
            (x + 12, 0.1, 1),
        ];
        
        for (base_x, offset, style) in flakes {
            let flake_phase = (phase * 1.5 + offset * TAU) % TAU;
            let progress = flake_phase / TAU;
            
            // Gentle horizontal drift
            let drift = ((flake_phase * 2.0).sin() * 2.0) as i32;
            let flake_x = base_x + drift;
            let flake_y = y + 1 + (progress * 12.0) as i32;
            
            if flake_y < y + 13 && flake_x >= x && flake_x < x + 14 {
                // Draw snowflake based on style
                self.canvas.set(flake_x, flake_y, true);
                
                if style == 0 {
                    // Simple cross +
                    self.canvas.set(flake_x - 1, flake_y, true);
                    self.canvas.set(flake_x + 1, flake_y, true);
                    self.canvas.set(flake_x, flake_y - 1, true);
                    self.canvas.set(flake_x, flake_y + 1, true);
                } else {
                    // Diagonal cross ×
                    self.canvas.set(flake_x - 1, flake_y - 1, true);
                    self.canvas.set(flake_x + 1, flake_y - 1, true);
                    self.canvas.set(flake_x - 1, flake_y + 1, true);
                    self.canvas.set(flake_x + 1, flake_y + 1, true);
                }
            }
        }
    }

    fn draw_unknown_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Stylized loading/question indicator
        let cx = x + 7;
        let cy = y + 6;
        
        // Spinning dots around center
        let dot_count = 6;
        let radius = 5.0;
        let brightness_phase = phase * 2.0;
        
        for i in 0..dot_count {
            let angle = (i as f32 / dot_count as f32) * TAU + phase * 2.0;
            let dx = (angle.cos() * radius) as i32;
            let dy = (angle.sin() * radius) as i32;
            
            // Each dot fades based on position in rotation
            let dot_brightness = ((i as f32 / dot_count as f32) * TAU + brightness_phase * 2.0).sin();
            if dot_brightness > -0.3 {
                self.canvas.set(cx + dx, cy + dy, true);
            }
        }
        
        // Center dot
        self.canvas.set(cx, cy, true);
    }

    fn draw_partly_cloudy_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Sun peeking behind a cloud
        let cx = x + 7;
        let cy = y + 6;
        
        // Small sun in upper-left with pulsing rays
        let sun_x = cx - 3;
        let sun_y = cy - 2;
        let pulse = ((phase * 2.0).sin() * 0.5 + 0.5) as i32;
        
        // Sun core (small)
        self.canvas.set(sun_x, sun_y, true);
        self.canvas.set(sun_x - 1, sun_y, true);
        self.canvas.set(sun_x + 1, sun_y, true);
        self.canvas.set(sun_x, sun_y - 1, true);
        self.canvas.set(sun_x, sun_y + 1, true);
        
        // Rays (pulsing)
        if pulse == 1 {
            self.canvas.set(sun_x - 2, sun_y - 1, true);
            self.canvas.set(sun_x + 2, sun_y - 1, true);
            self.canvas.set(sun_x - 2, sun_y + 1, true);
        }
        
        // Cloud in front (lower-right), with bob
        let bob = ((phase * 1.2).sin() * 1.5) as i32;
        let cloud_y = cy + 2 + bob;
        
        #[rustfmt::skip]
        const SMALL_CLOUD: [[u8; 10]; 5] = [
            [0,0,0,1,1,1,0,0,0,0],
            [0,0,1,1,1,1,1,0,0,0],
            [0,1,1,1,1,1,1,1,0,0],
            [1,1,1,1,1,1,1,1,1,0],
            [0,1,1,1,1,1,1,1,0,0],
        ];
        
        for (row, cols) in SMALL_CLOUD.iter().enumerate() {
            for (col, &px) in cols.iter().enumerate() {
                if px == 1 {
                    self.canvas.set(cx - 3 + col as i32, cloud_y - 2 + row as i32, true);
                }
            }
        }
    }

    fn draw_fog_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Horizontal fog lines that drift
        let cx = x + 7;
        
        // Multiple horizontal lines with varying drift
        let lines = [
            (y + 3, 0.0, 10),
            (y + 5, 0.3, 12),
            (y + 7, 0.6, 10),
            (y + 9, 0.9, 8),
            (y + 11, 0.2, 10),
        ];
        
        for (line_y, offset, width) in lines {
            let drift = ((phase * 0.8 + offset * TAU).sin() * 2.0) as i32;
            let start_x = cx - width / 2 + drift;
            
            // Dashed fog line with gaps
            for i in 0..width {
                if i % 3 != 2 { // Create small gaps
                    self.canvas.set(start_x + i, line_y, true);
                }
            }
        }
    }

    fn draw_drizzle_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Light cloud with small diagonal rain
        let cx = x + 7;
        let cy = y + 3;
        
        // Small cloud
        #[rustfmt::skip]
        const DRIZZLE_CLOUD: [[u8; 10]; 4] = [
            [0,0,0,1,1,1,0,0,0,0],
            [0,0,1,1,1,1,1,0,0,0],
            [0,1,1,1,1,1,1,1,0,0],
            [1,1,1,1,1,1,1,1,1,0],
        ];
        
        for (row, cols) in DRIZZLE_CLOUD.iter().enumerate() {
            for (col, &px) in cols.iter().enumerate() {
                if px == 1 {
                    self.canvas.set(cx - 5 + col as i32, cy - 1 + row as i32, true);
                }
            }
        }
        
        // Light diagonal drizzle drops (smaller, more spaced)
        let drops = [
            (cx - 3, 0.0),
            (cx + 1, 0.5),
            (cx + 4, 0.25),
        ];
        
        for (drop_x, offset) in drops {
            let drop_phase = (phase * 2.0 + offset * TAU) % TAU;
            let progress = drop_phase / TAU;
            let drop_y = cy + 4 + (progress * 7.0) as i32;
            
            if drop_y < y + 12 {
                // Single pixel drops (light drizzle)
                self.canvas.set(drop_x, drop_y, true);
            }
        }
    }

    fn draw_thunderstorm_icon(&mut self, x: i32, y: i32, _size: i32, phase: f32) {
        // Dark cloud with lightning bolt
        let cx = x + 7;
        let cy = y + 3;
        
        // Compact storm cloud
        #[rustfmt::skip]
        const STORM_CLOUD: [[u8; 12]; 5] = [
            [0,0,0,1,1,1,1,1,0,0,0,0],
            [0,0,1,1,1,1,1,1,1,0,0,0],
            [0,1,1,1,1,1,1,1,1,1,0,0],
            [1,1,1,1,1,1,1,1,1,1,1,0],
            [0,1,1,1,1,1,1,1,1,1,0,0],
        ];
        
        for (row, cols) in STORM_CLOUD.iter().enumerate() {
            for (col, &px) in cols.iter().enumerate() {
                if px == 1 {
                    self.canvas.set(cx - 6 + col as i32, cy - 2 + row as i32, true);
                }
            }
        }
        
        // Flashing lightning bolt
        let flash = ((phase * 4.0).sin() > 0.3) as i32;
        if flash == 1 {
            // Lightning bolt shape: zigzag
            //   ##
            //  ##
            //   #
            //  ##
            //   #
            let bolt_x = cx - 1;
            let bolt_y = cy + 4;
            
            self.canvas.set(bolt_x + 1, bolt_y, true);
            self.canvas.set(bolt_x + 2, bolt_y, true);
            self.canvas.set(bolt_x, bolt_y + 1, true);
            self.canvas.set(bolt_x + 1, bolt_y + 1, true);
            self.canvas.set(bolt_x + 1, bolt_y + 2, true);
            self.canvas.set(bolt_x, bolt_y + 3, true);
            self.canvas.set(bolt_x + 1, bolt_y + 3, true);
            self.canvas.set(bolt_x + 1, bolt_y + 4, true);
        }
    }

    fn draw_volume(&mut self, widget: &Widget, sample: &MetricsSample) {
        let current_volume = (sample.volume_percent.round() as i32).clamp(0, 100);
        self.update_volume_animation(current_volume);

        self.draw_bar(
            &widget.position,
            sample.volume_percent,
            widget
                .bar
                .as_ref()
                .map(|b| b.direction.as_str())
                .unwrap_or("horizontal"),
            widget.bar.as_ref().map(|b| b.border).unwrap_or(true),
        );

        if widget.show_icon {
            let p = &widget.position;
            let cx = p.x + 2;                    // left edge of icon
            let top = p.y + 3;                    // 2px from border (1px border + 2px gap)
            let bot = p.y + p.h - 4;             // 2px from border
            let cy = p.y + p.h / 2;              // vertical center
            let half = (bot - top) / 2;           // half-height of icon

            // Speaker body: rectangle (driver) — ~1/3 of total width
            let body_w = 3;
            let body_half = half * 2 / 3;         // body is shorter than cone
            self.canvas.rect_fill_invert(cx, cy - body_half, body_w, body_half * 2 + 1);

            // Cone: triangle expanding right from the driver
            self.canvas.line_invert(cx + body_w, cy - body_half, cx + body_w + 3, top);
            self.canvas.line_invert(cx + body_w, cy + body_half, cx + body_w + 3, bot);
            self.canvas.line_invert(cx + body_w + 3, top, cx + body_w + 3, bot);

            // Sound wave arcs — count based on volume level
            // 0% (mute) = 0 waves, 1-33% = 1, 34-66% = 2, 67-100% = 3
            let vol = sample.volume_percent;
            let wave_count = if vol <= 0.0 { 0 } else if vol <= 33.0 { 1 } else if vol <= 66.0 { 2 } else { 3 };

            if wave_count >= 1 {
                let w1_x = cx + body_w + 5;
                let w1_h = half / 3;
                for dy in -w1_h..=w1_h {
                    self.canvas.invert(w1_x, cy + dy);
                }
            }
            if wave_count >= 2 {
                let w2_x = cx + body_w + 7;
                let w2_h = half * 2 / 3;
                for dy in -w2_h..=w2_h {
                    self.canvas.invert(w2_x, cy + dy);
                }
            }
            if wave_count >= 3 {
                let w3_x = cx + body_w + 9;
                for dy in -half..=half {
                    self.canvas.invert(w3_x, cy + dy);
                }
            }
        }

        let scale = 2;
        let p = &widget.position;
        let border = widget.bar.as_ref().map(|b| b.border).unwrap_or(true);
        let char_w = 5 * scale;
        let text_px = 4 * char_w; // 3 digits + %
        let text_h = 5 * scale;
        let left_bound = p.x + if widget.show_icon { 14 } else { 1 };
        let right_bound = p.x + p.w - 2;
        let mut text_x = right_bound - text_px + 1;
        if text_x < left_bound {
            text_x = left_bound;
        }
        let base_y = p.y + ((p.h - text_h) / 2).max(0);

        let clip_x = if border { p.x + 1 } else { p.x };
        let clip_y = if border { p.y + 1 } else { p.y };
        let clip_w = if border { p.w - 2 } else { p.w };
        let clip_h = if border { p.h - 2 } else { p.h };

        let text_clip_y = base_y.max(clip_y);
        let text_clip_bottom = (base_y + text_h - 1).min(clip_y + clip_h - 1);
        let text_clip_h = (text_clip_bottom - text_clip_y + 1).max(0);

        if self.vol_anim_step < self.vol_anim_len && self.vol_step_from != self.vol_step_to {
            let increasing = self.vol_step_to > self.vol_step_from;
            let dir = if increasing { -1 } else { 1 }; // increase rolls up, decrease rolls down

            let old_digits = Self::volume_digits(self.vol_step_from);
            let new_digits = Self::volume_digits(self.vol_step_to);

            let leave_frames = (self.vol_anim_len / 2).max(1);
            let enter_frames = self
                .vol_anim_len
                .saturating_sub(leave_frames)
                .max(1);

            for i in 0..3 {
                let slot_x = text_x + i as i32 * char_w;
                let slot_clip_x = slot_x.max(clip_x);
                let slot_clip_right = (slot_x + char_w - 1).min(clip_x + clip_w - 1);
                let slot_clip_w = (slot_clip_right - slot_clip_x + 1).max(0);
                let from_ch = old_digits[i];
                let to_ch = new_digits[i];

                if from_ch == to_ch {
                    self.canvas.draw_char_scaled_invert_clipped(
                        slot_x,
                        base_y,
                        to_ch,
                        scale,
                        slot_clip_x,
                        text_clip_y,
                        slot_clip_w,
                        text_clip_h,
                    );
                    continue;
                }

                if self.vol_anim_step < leave_frames {
                    let step = self.vol_anim_step as i32 + 1;
                    let offset = (step * text_h) / leave_frames as i32;
                    self.canvas.draw_char_scaled_invert_clipped(
                        slot_x,
                        base_y + dir * offset,
                        from_ch,
                        scale,
                        slot_clip_x,
                        text_clip_y,
                        slot_clip_w,
                        text_clip_h,
                    );
                } else {
                    let step = (self.vol_anim_step - leave_frames) as i32 + 1;
                    let enter_frames = enter_frames.max(1) as i32;
                    let offset = text_h - (step * text_h) / enter_frames;
                    self.canvas.draw_char_scaled_invert_clipped(
                        slot_x,
                        base_y - dir * offset,
                        to_ch,
                        scale,
                        slot_clip_x,
                        text_clip_y,
                        slot_clip_w,
                        text_clip_h,
                    );
                }
            }

            // Percent sign remains static / unanimated.
            let percent_x = text_x + 3 * char_w;
            let percent_clip_x = percent_x.max(clip_x);
            let percent_clip_right = (percent_x + char_w - 1).min(clip_x + clip_w - 1);
            let percent_clip_w = (percent_clip_right - percent_clip_x + 1).max(0);
            self.canvas.draw_char_scaled_invert_clipped(
                percent_x,
                base_y,
                '%',
                scale,
                percent_clip_x,
                text_clip_y,
                percent_clip_w,
                text_clip_h,
            );
        } else {
            let shown = self.volume_display.unwrap_or(current_volume);
            let digits = Self::volume_digits(shown);
            for (i, ch) in digits.iter().enumerate() {
                let slot_x = text_x + i as i32 * char_w;
                let slot_clip_x = slot_x.max(clip_x);
                let slot_clip_right = (slot_x + char_w - 1).min(clip_x + clip_w - 1);
                let slot_clip_w = (slot_clip_right - slot_clip_x + 1).max(0);
                self.canvas.draw_char_scaled_invert_clipped(
                    slot_x,
                    base_y,
                    *ch,
                    scale,
                    slot_clip_x,
                    text_clip_y,
                    slot_clip_w,
                    text_clip_h,
                );
            }
            let percent_x = text_x + 3 * char_w;
            let percent_clip_x = percent_x.max(clip_x);
            let percent_clip_right = (percent_x + char_w - 1).min(clip_x + clip_w - 1);
            let percent_clip_w = (percent_clip_right - percent_clip_x + 1).max(0);
            self.canvas.draw_char_scaled_invert_clipped(
                percent_x,
                base_y,
                '%',
                scale,
                percent_clip_x,
                text_clip_y,
                percent_clip_w,
                text_clip_h,
            );
        }

        self.advance_volume_animation();
    }

    fn volume_digits(value: i32) -> [char; 3] {
        let s = format!("{:>3}", value.clamp(0, 100));
        let mut chars = s.chars();
        [
            chars.next().unwrap_or(' '),
            chars.next().unwrap_or(' '),
            chars.next().unwrap_or(' '),
        ]
    }

    fn draw_memory(&mut self, widget: &Widget, sample: &MetricsSample) {
        let history_len = widget
            .graph
            .as_ref()
            .map(|g| g.history)
            .unwrap_or(widget.position.w.max(1) as usize)
            .max(2);

        self.mem_history.push_back(sample.mem_percent);
        while self.mem_history.len() > history_len {
            self.mem_history.pop_front();
        }

        let history = self.mem_history.clone();
        self.draw_graph(&widget.position, &history);
        let text = format!("{:>3}%", sample.mem_percent.round() as i32);
        let char_w = 5; // tiny font width
        let text_px = text.len() as i32 * char_w;
        let text_x = widget.position.x + widget.position.w - text_px - 1;
        self.canvas
            .draw_text_tiny(text_x, widget.position.y + 1, &text);
    }

    fn draw_network(&mut self, widget: &Widget, sample: &MetricsSample) {
        let p = &widget.position;
        let down = human_speed(sample.net_down_bps);
        let up = human_speed(sample.net_up_bps);

        let char_w = 5; // tiny font: 4px glyph + 1px gap
        let right_edge = p.x + p.w - char_w; // unit char right-aligned

        // Split value and unit (unit is always last char)
        let (up_val, up_unit) = up.split_at(up.len() - 1);
        let (dn_val, dn_unit) = down.split_at(down.len() - 1);

        self.canvas.draw_text_tiny(p.x + 1, p.y + 1, &format!("U {up_val}"));
        self.canvas.draw_text_tiny(right_edge, p.y + 1, up_unit);

        self.canvas.draw_text_tiny(p.x + 1, p.y + 10, &format!("D {dn_val}"));
        self.canvas.draw_text_tiny(right_edge, p.y + 10, dn_unit);
    }

    fn draw_keyboard(&mut self, widget: &Widget, sample: &MetricsSample) {
        let _ = widget;

        self.update_capslock_animation(sample.caps_lock);
        self.update_numlock_animation(sample.num_lock);
        self.update_scrolllock_animation(sample.scroll_lock);

        let icon_w = 9;
        let gap = 1;
        let total_w = icon_w * 3 + gap * 2;
        let start_x = (self.width as i32 - total_w - 1).max(0);
        let y = 1;

        // Caps Lock: up arrow
        let caps_anim = if self.caps_anim_step < self.caps_anim_len {
            Some((
                self.caps_anim_from,
                self.caps_anim_to,
                self.caps_anim_step,
                self.caps_anim_len,
            ))
        } else {
            None
        };
        self.draw_chevron(start_x, y, icon_w, true, sample.caps_lock, caps_anim);
        if caps_anim.is_some() {
            self.caps_anim_step = self.caps_anim_step.saturating_add(1);
        }

        // Num Lock: padlock
        let num_x = start_x + icon_w + gap;
        let num_anim = if self.num_anim_step < self.num_anim_len {
            Some((
                self.num_anim_from,
                self.num_anim_to,
                self.num_anim_step,
                self.num_anim_len,
            ))
        } else {
            None
        };
        self.draw_padlock(num_x, y, icon_w, sample.num_lock, num_anim);
        if num_anim.is_some() {
            self.num_anim_step = self.num_anim_step.saturating_add(1);
        }

        // Scroll Lock: down arrow
        let scrl_x = num_x + icon_w + gap;
        let scroll_anim = if self.scroll_anim_step < self.scroll_anim_len {
            Some((
                self.scroll_anim_from,
                self.scroll_anim_to,
                self.scroll_anim_step,
                self.scroll_anim_len,
            ))
        } else {
            None
        };
        self.draw_chevron(scrl_x, y, icon_w, false, sample.scroll_lock, scroll_anim);
        if scroll_anim.is_some() {
            self.scroll_anim_step = self.scroll_anim_step.saturating_add(1);
        }
    }

    fn chevron_bitmap(up: bool, on: bool) -> [u16; 10] {
        if up {
            if on {
                [
                    0x010, // ....X....
                    0x038, // ...XXX...
                    0x07C, // ..XXXXX..
                    0x0FE, // .XXXXXXX.
                    0x1FF, // XXXXXXXXX
                    0x038, // ...XXX...
                    0x038, // ...XXX...
                    0x038, // ...XXX...
                    0x038, // ...XXX...
                    0x038, // ...XXX...
                ]
            } else {
                [
                    0x010, // ....X....
                    0x028, // ...X.X...
                    0x044, // ..X...X..
                    0x082, // .X.....X.
                    0x1EF, // XXXX.XXXX
                    0x028, // ...X.X...
                    0x028, // ...X.X...
                    0x028, // ...X.X...
                    0x028, // ...X.X...
                    0x038, // ...XXX...
                ]
            }
        } else if on {
            [
                0x038, // ...XXX...
                0x038, // ...XXX...
                0x038, // ...XXX...
                0x038, // ...XXX...
                0x038, // ...XXX...
                0x1FF, // XXXXXXXXX
                0x0FE, // .XXXXXXX.
                0x07C, // ..XXXXX..
                0x038, // ...XXX...
                0x010, // ....X....
            ]
        } else {
            [
                0x038, // ...XXX...
                0x028, // ...X.X...
                0x028, // ...X.X...
                0x028, // ...X.X...
                0x028, // ...X.X...
                0x1EF, // XXXX.XXXX
                0x082, // .X.....X.
                0x044, // ..X...X..
                0x028, // ...X.X...
                0x010, // ....X....
            ]
        }
    }

    /// Arrow using handcrafted 9×10 pixel bitmaps.
    /// OFF = outline only, ON = solid filled.
    fn draw_chevron(
        &mut self,
        x: i32,
        y: i32,
        _w: i32,
        up: bool,
        on: bool,
        anim: Option<(bool, bool, u8, u8)>,
    ) {
        // Each row is a u16 bitmask, bit 0 = leftmost pixel, 9 pixels wide.
        let (bitmap, y_shift): ([u16; 10], i32) = if let Some((from_on, to_on, step, len)) = anim {
            let t = if len == 0 {
                1.0
            } else {
                (step as f32 / len as f32).clamp(0.0, 1.0)
            };

            let from = Self::chevron_bitmap(up, from_on);
            let to = Self::chevron_bitmap(up, to_on);
            let mut blended = from;

            // Transition from center outward: center rows switch first.
            let center_row = 4i32;
            let radius = ((t * 5.0).round() as i32).clamp(0, 5);
            for row in 0..10 {
                if (row as i32 - center_row).abs() <= radius {
                    blended[row] = to[row];
                }
            }

            // OFF transition always bounces downward.
            // ON transition keeps directional glide by arrow orientation.
            let shift_mag = ((1.0 - t) * 3.0).round() as i32;
            let shift = if !to_on {
                shift_mag
            } else if up {
                -shift_mag
            } else {
                shift_mag
            };

            (blended, shift)
        } else {
            (Self::chevron_bitmap(up, on), 0)
        };

        for (row, &bits) in bitmap.iter().enumerate() {
            for col in 0..9i32 {
                if (bits >> col) & 1 == 1 {
                    self.canvas.set(x + col, y + y_shift + row as i32, true);
                }
            }
        }
    }

    fn update_capslock_animation(&mut self, now: bool) {
        if let Some(prev) = self.prev_caps_lock
            && prev != now
        {
            self.caps_anim_from = prev;
            self.caps_anim_to = now;
            self.caps_anim_step = 0;
        }
        self.prev_caps_lock = Some(now);
    }

    fn update_volume_animation(&mut self, now: i32) {
        if self.volume_display.is_none() {
            self.volume_display = Some(now);
            self.volume_target = now;
            self.vol_step_from = now;
            self.vol_step_to = now;
            self.vol_anim_step = self.vol_anim_len;
            return;
        }

        self.volume_target = now;

        if self.vol_anim_step >= self.vol_anim_len && self.vol_step_from == self.vol_step_to {
            let display = self.volume_display.unwrap_or(now);
            if display != self.volume_target {
                self.vol_step_from = display;
                self.vol_step_to = display + if self.volume_target > display { 1 } else { -1 };
                self.vol_anim_step = 0;
            }
        }
    }

    fn advance_volume_animation(&mut self) {
        let display = self.volume_display.unwrap_or(self.volume_target);
        let distance = (self.volume_target - display).unsigned_abs();
        let distance_bonus = match distance {
            0..=2 => 0,
            3..=8 => 1,
            _ => 2,
        };
        let speed = self
            .vol_anim_speed
            .saturating_add(distance_bonus)
            .clamp(1, 3);

        for _ in 0..speed {
            if self.vol_step_from == self.vol_step_to || self.vol_anim_step >= self.vol_anim_len {
                let display = self.volume_display.unwrap_or(self.volume_target);
                if display != self.volume_target {
                    self.vol_step_from = display;
                    self.vol_step_to = display + if self.volume_target > display { 1 } else { -1 };
                    self.vol_anim_step = 0;
                }
                continue;
            }

            self.vol_anim_step = self.vol_anim_step.saturating_add(1);
            if self.vol_anim_step >= self.vol_anim_len {
                self.volume_display = Some(self.vol_step_to);

                if self.vol_step_to != self.volume_target {
                    self.vol_step_from = self.vol_step_to;
                    self.vol_step_to += if self.volume_target > self.vol_step_to { 1 } else { -1 };
                    self.vol_anim_step = 0;
                } else {
                    self.vol_step_from = self.vol_step_to;
                }
            }
        }
    }

    fn update_numlock_animation(&mut self, now: bool) {
        if let Some(prev) = self.prev_num_lock
            && prev != now
        {
            self.num_anim_from = prev;
            self.num_anim_to = now;
            self.num_anim_step = 0;
        }
        self.prev_num_lock = Some(now);
    }

    fn update_scrolllock_animation(&mut self, now: bool) {
        if let Some(prev) = self.prev_scroll_lock
            && prev != now
        {
            self.scroll_anim_from = prev;
            self.scroll_anim_to = now;
            self.scroll_anim_step = 0;
        }
        self.prev_scroll_lock = Some(now);
    }

    fn padlock_bitmap(on: bool) -> [u16; 10] {
        if on {
            [
                0x03C, // ..XXXX...
                0x044, // ..X...X..
                0x044, // ..X...X..
                0x044, // ..X...X..
                0x1FF, // XXXXXXXXX
                0x1FF, // XXXXXXXXX
                0x1EF, // XXXX.XXXX
                0x1EF, // XXXX.XXXX
                0x1FF, // XXXXXXXXX
                0x1FF, // XXXXXXXXX
            ]
        } else {
            [
                0x03C, // ..XXXX...
                0x004, // ..X......
                0x004, // ..X......
                0x004, // ..X......
                0x1FF, // XXXXXXXXX
                0x101, // X.......X
                0x101, // X.......X
                0x111, // X...X...X
                0x101, // X.......X
                0x1FF, // XXXXXXXXX
            ]
        }
    }

    /// Padlock animation mirrors chevron animation style:
    /// center-out bitmap transition plus vertical glide.
    fn draw_padlock(
        &mut self,
        x: i32,
        y: i32,
        _w: i32,
        on: bool,
        anim: Option<(bool, bool, u8, u8)>,
    ) {
        let (bitmap, y_shift): ([u16; 10], i32) = if let Some((from_on, to_on, step, len)) = anim {
            let t = if len == 0 {
                1.0
            } else {
                (step as f32 / len as f32).clamp(0.0, 1.0)
            };

            let from = Self::padlock_bitmap(from_on);
            let to = Self::padlock_bitmap(to_on);
            let mut blended = from;

            let center_row = 4i32;
            let radius = ((t * 5.0).round() as i32).clamp(0, 5);
            for row in 0..10 {
                if (row as i32 - center_row).abs() <= radius {
                    blended[row] = to[row];
                }
            }

            // Ordered right-shackle-leg transition:
            // OFF: bottom pixel dissolves first, then top pixel.
            // ON: top pixel reappears first, then bottom pixel.
            let right_leg_col = 6u16;
            let bottom_row = 3usize;
            let top_row = 1usize;
            let stage = if len <= 1 {
                2
            } else {
                ((step as i32 * 3) / len as i32).clamp(0, 2)
            };

            if !to_on {
                if stage >= 1 {
                    blended[bottom_row] &= !(1u16 << right_leg_col);
                }
                if stage >= 2 {
                    blended[top_row] &= !(1u16 << right_leg_col);
                }
            } else {
                if stage >= 1 {
                    blended[top_row] |= 1u16 << right_leg_col;
                }
                if stage >= 2 {
                    blended[bottom_row] |= 1u16 << right_leg_col;
                }
            }

            let shift_mag = ((1.0 - t) * 3.0).round() as i32;
            let shift = if to_on { shift_mag } else { -shift_mag };

            (blended, shift)
        } else {
            (Self::padlock_bitmap(on), 0)
        };

        for (row, &bits) in bitmap.iter().enumerate() {
            for col in 0..9i32 {
                if (bits >> col) & 1 == 1 {
                    self.canvas.set(x + col, y + y_shift + row as i32, true);
                }
            }
        }
    }

    fn draw_bar(&mut self, pos: &Position, percent: f32, direction: &str, border: bool) {
        let p = percent.clamp(0.0, 100.0);

        if border {
            self.canvas.rect_border(pos.x, pos.y, pos.w, pos.h, true);
        }

        let inner_x = if border { pos.x + 1 } else { pos.x };
        let inner_y = if border { pos.y + 1 } else { pos.y };
        let inner_w = if border { pos.w - 2 } else { pos.w };
        let inner_h = if border { pos.h - 2 } else { pos.h };

        if inner_w <= 0 || inner_h <= 0 {
            return;
        }

        if direction == "vertical" {
            let fill_h = ((inner_h as f32) * (p / 100.0)).round() as i32;
            let y = inner_y + (inner_h - fill_h);
            self.canvas.rect_fill(inner_x, y, inner_w, fill_h, true);
        } else {
            let fill_w = ((inner_w as f32) * (p / 100.0)).round() as i32;
            self.canvas.rect_fill(inner_x, inner_y, fill_w, inner_h, true);
        }
    }

    fn draw_graph(&mut self, pos: &Position, history: &VecDeque<f32>) {
        if history.len() < 2 || pos.w <= 1 || pos.h <= 1 {
            return;
        }

        let len = history.len();
        let bottom = pos.y + pos.h - 1;

        // Collect graph Y for each column via linear interpolation between sample points
        let mut col_y: Vec<i32> = Vec::with_capacity(pos.w as usize);
        let mut prev_x = pos.x;
        let mut prev_vy = pos.y + pos.h - 1 - ((history[0] / 100.0) * (pos.h - 1) as f32) as i32;

        for (i, value) in history.iter().enumerate().take(len).skip(1) {
            let x = pos.x + ((i as i32) * (pos.w - 1) / (len as i32 - 1));
            let vy = pos.y + pos.h - 1 - ((*value / 100.0) * (pos.h - 1) as f32) as i32;

            // Interpolate columns between prev_x and x
            let dx = x - prev_x;
            for cx in prev_x..=x {
                let t = if dx == 0 { 0.0 } else { (cx - prev_x) as f32 / dx as f32 };
                let line_y = prev_vy as f32 + t * (vy - prev_vy) as f32;
                col_y.push(line_y.round() as i32);
            }

            prev_x = x + 1; // avoid duplicate column
            prev_vy = vy;
        }

        // Fill below line with checkerboard dither, then draw the line itself
        for (ci, &ly) in col_y.iter().enumerate() {
            let cx = pos.x + ci as i32;
            // Dithered fill: from line_y+1 down to bottom
            for fy in (ly + 1)..=bottom {
                if (cx + fy) % 2 == 0 {
                    self.canvas.set(cx, fy, true);
                }
            }
            // Solid line pixel
            self.canvas.set(cx, ly, true);
        }
    }
}

fn human_speed(bytes_per_sec: f64) -> String {
    const UNITS: [char; 4] = ['B', 'K', 'M', 'G'];

    let mut value = bytes_per_sec.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{:.0}{}", value, UNITS[unit])
    } else {
        format!("{:.1}{}", value, UNITS[unit])
    }
}
