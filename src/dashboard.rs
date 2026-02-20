use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::time::{Duration, Instant};
use crate::canvas::Canvas;
use crate::config::{DashboardConfig, Position, Widget};
use crate::metrics::MetricsSample;

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
        }
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
                "volume" => self.draw_volume(widget, sample),
                "memory" => self.draw_memory(widget, sample),
                "network" => self.draw_network(widget, sample),
                "keyboard" => self.draw_keyboard(widget, sample),
                _ => {}
            }
        }

        self.canvas.to_packed_bytes()
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
            let handoff_frames = 1u8;
            let enter_frames = self
                .vol_anim_len
                .saturating_sub(leave_frames)
                .saturating_sub(handoff_frames)
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
                } else if self.vol_anim_step < leave_frames + handoff_frames {
                    continue;
                } else {
                    let step = (self.vol_anim_step - leave_frames - handoff_frames) as i32 + 1;
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
        self.draw_padlock(num_x, y, icon_w, sample.num_lock);

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

    /// A padlock icon: rounded shackle on top, rectangular body below.
    /// Animated shackle open/close on toggle.
    fn draw_padlock(&mut self, x: i32, y: i32, w: i32, on: bool) {
        let mut openness = if on { 0 } else { 3 };

        if self.num_anim_step < self.num_anim_len {
            let from = if self.num_anim_from { 0.0 } else { 3.0 };
            let to = if self.num_anim_to { 0.0 } else { 3.0 };
            let t = self.num_anim_step as f32 / self.num_anim_len as f32;
            openness = (from + (to - from) * t).round() as i32;
            self.num_anim_step = self.num_anim_step.saturating_add(1);
        }

        let body_x = x + 1;
        let body_y = y + 6;
        let body_w = w - 2;
        let body_h = 5;

        // Lock body
        if on {
            self.canvas.rect_fill(body_x, body_y, body_w, body_h, true);
            // Keyhole: dark dot in center of filled body
            self.canvas.set(x + w / 2, body_y + 2, false);
        } else {
            self.canvas.rect_border(body_x, body_y, body_w, body_h, true);
        }

        // Shackle: U-shape above body
        let shackle_top = y + 2 - openness;
        let left = x + 2;
        let right = x + w - 3;

        // Left side of shackle
        self.canvas.line(left, body_y, left, shackle_top + 1, true);
        // Top arc (flat top + corner pixels for roundness)
        self.canvas.line(left + 1, shackle_top, right - 1, shackle_top, true);
        // Right side — only draws when closed/closing
        if openness <= 1 {
            self.canvas.line(right, shackle_top + 1, right, body_y, true);
        } else {
            // Partial right side when opening
            self.canvas.line(right, shackle_top + openness, right, shackle_top + openness + 1, true);
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
