use std::collections::VecDeque;

use crate::canvas::Canvas;
use crate::config::{DashboardConfig, Position, Widget};
use crate::metrics::MetricsSample;

pub struct DashboardRenderer {
    canvas: Canvas,
    width: usize,
    mem_history: VecDeque<f32>,
    prev_num_lock: Option<bool>,
    num_anim_step: u8,
    num_anim_len: u8,
    num_anim_from: bool,
    num_anim_to: bool,
}

impl DashboardRenderer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            canvas: Canvas::new(width, height),
            width,
            mem_history: VecDeque::new(),
            prev_num_lock: None,
            num_anim_step: 0,
            num_anim_len: 6,
            num_anim_from: false,
            num_anim_to: false,
        }
    }

    pub fn render(&mut self, config: &DashboardConfig, sample: &MetricsSample) -> Vec<u8> {
        self.canvas.clear(config.display.background > 0);

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
        let text = format!("{:>3}%", sample.volume_percent.round() as i32);
        let p = &widget.position;
        let char_w = 5 * scale;
        let text_px = (text.len() as i32) * char_w;
        let text_h = 5 * scale;
        let left_bound = p.x + if widget.show_icon { 14 } else { 1 };
        let right_bound = p.x + p.w - 2;
        let mut text_x = right_bound - text_px + 1;
        if text_x < left_bound {
            text_x = left_bound;
        }
        let text_y = p.y + ((p.h - text_h) / 2).max(0);
        self.canvas.draw_text_scaled_invert(text_x, text_y, &text, scale);
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

        self.update_numlock_animation(sample.num_lock);

        let icon_w = 9;
        let gap = 1;
        let total_w = icon_w * 3 + gap * 2;
        let start_x = (self.width as i32 - total_w - 1).max(0);
        let y = 1;

        // Caps Lock: up arrow
        self.draw_chevron(start_x, y, icon_w, true, sample.caps_lock);

        // Num Lock: padlock
        let num_x = start_x + icon_w + gap;
        self.draw_padlock(num_x, y, icon_w, sample.num_lock);

        // Scroll Lock: down arrow
        let scrl_x = num_x + icon_w + gap;
        self.draw_chevron(scrl_x, y, icon_w, false, sample.scroll_lock);
    }

    /// Arrow using handcrafted 9×10 pixel bitmaps.
    /// OFF = outline only, ON = solid filled.
    fn draw_chevron(&mut self, x: i32, y: i32, _w: i32, up: bool, on: bool) {
        // Each row is a u16 bitmask, bit 0 = leftmost pixel, 9 pixels wide.
        let bitmap: &[u16; 10] = if up {
            if on {
                // Solid up arrow
                &[
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
                // Outline up arrow
                &[
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
        } else {
            if on {
                // Solid down arrow
                &[
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
                // Outline down arrow
                &[
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
        };

        for (row, &bits) in bitmap.iter().enumerate() {
            for col in 0..9i32 {
                if (bits >> col) & 1 == 1 {
                    self.canvas.set(x + col, y + row as i32, true);
                }
            }
        }
    }

    fn update_numlock_animation(&mut self, now: bool) {
        if let Some(prev) = self.prev_num_lock {
            if prev != now {
                self.num_anim_from = prev;
                self.num_anim_to = now;
                self.num_anim_step = 0;
            }
        }
        self.prev_num_lock = Some(now);
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

        for i in 1..len {
            let x = pos.x + ((i as i32) * (pos.w - 1) / (len as i32 - 1));
            let vy = pos.y + pos.h - 1 - ((history[i] / 100.0) * (pos.h - 1) as f32) as i32;

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
