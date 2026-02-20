pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    pub fn clear(&mut self, on: bool) {
        self.pixels.fill(if on { 1 } else { 0 });
    }

    pub fn set(&mut self, x: i32, y: i32, on: bool) {
        if x < 0 || y < 0 {
            return;
        }
        let ux = x as usize;
        let uy = y as usize;
        if ux >= self.width || uy >= self.height {
            return;
        }
        self.pixels[uy * self.width + ux] = u8::from(on);
    }

    pub fn invert(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 {
            return;
        }
        let ux = x as usize;
        let uy = y as usize;
        if ux >= self.width || uy >= self.height {
            return;
        }
        let idx = uy * self.width + ux;
        self.pixels[idx] ^= 1;
    }

    pub fn rect_fill(&mut self, x: i32, y: i32, w: i32, h: i32, on: bool) {
        for py in y..(y + h) {
            for px in x..(x + w) {
                self.set(px, py, on);
            }
        }
    }

    pub fn rect_border(&mut self, x: i32, y: i32, w: i32, h: i32, on: bool) {
        for px in x..(x + w) {
            self.set(px, y, on);
            self.set(px, y + h - 1, on);
        }
        for py in y..(y + h) {
            self.set(x, py, on);
            self.set(x + w - 1, py, on);
        }
    }

    pub fn line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, on: bool) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.set(x0, y0, on);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    pub fn line_invert(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.invert(x0, y0);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    pub fn rect_fill_invert(&mut self, x: i32, y: i32, w: i32, h: i32) {
        for py in y..(y + h) {
            for px in x..(x + w) {
                self.invert(px, py);
            }
        }
    }

    /// Draw text using the built-in 4×5 pixel font at the given integer scale.
    /// At scale=1: 4×5 glyphs, 5px advance. At scale=2: 8×10 glyphs, 10px advance.
    pub fn draw_text_scaled(&mut self, x: i32, y: i32, text: &str, scale: i32) {
        let s = scale.max(1);
        let advance = 5 * s;
        let mut cursor_x = x;
        for ch in text.chars() {
            if let Some(glyph) = tiny_glyph(ch) {
                for (row, &bits) in glyph.iter().enumerate() {
                    for col in 0..4i32 {
                        if (bits >> col) & 1 == 1 {
                            for dy in 0..s {
                                for dx in 0..s {
                                    self.set(cursor_x + col * s + dx, y + row as i32 * s + dy, true);
                                }
                            }
                        }
                    }
                }
            }
            cursor_x += advance;
        }
    }

    /// Draw text using the built-in 4×5 pixel font at the given scale, inverting pixels.
    pub fn draw_text_scaled_invert(&mut self, x: i32, y: i32, text: &str, scale: i32) {
        let s = scale.max(1);
        let advance = 5 * s;
        let mut cursor_x = x;
        for ch in text.chars() {
            if let Some(glyph) = tiny_glyph(ch) {
                for (row, &bits) in glyph.iter().enumerate() {
                    for col in 0..4i32 {
                        if (bits >> col) & 1 == 1 {
                            for dy in 0..s {
                                for dx in 0..s {
                                    self.invert(cursor_x + col * s + dx, y + row as i32 * s + dy);
                                }
                            }
                        }
                    }
                }
            }
            cursor_x += advance;
        }
    }

    /// Convenience: draw at scale 1 (4×5 native size).
    pub fn draw_text_tiny(&mut self, x: i32, y: i32, text: &str) {
        self.draw_text_scaled(x, y, text, 1);
    }

    pub fn to_packed_bytes(&self) -> Vec<u8> {
        let mut out = vec![0u8; (self.width * self.height).div_ceil(8)];

        let mut byte_index = 0;
        let mut bit_index = 0;
        let mut current = 0u8;

        for y in 0..self.height {
            for x in 0..self.width {
                if self.pixels[y * self.width + x] > 0 {
                    current |= 1 << (7 - bit_index);
                }

                bit_index += 1;
                if bit_index == 8 {
                    out[byte_index] = current;
                    byte_index += 1;
                    bit_index = 0;
                    current = 0;
                }
            }
        }

        if bit_index > 0 {
            out[byte_index] = current;
        }

        out
    }
}

/// 4×5 pixel bitmap font with 1px-thick strokes.
/// Each entry is 5 rows; in each row, bit N = column N (bit 0 = leftmost).
fn tiny_glyph(ch: char) -> Option<[u8; 5]> {
    let ch = ch.to_ascii_uppercase();
    Some(match ch {
        '0' => [0b0110, 0b1001, 0b1001, 0b1001, 0b0110],
        '1' => [0b0010, 0b0011, 0b0010, 0b0010, 0b0111],
        '2' => [0b0110, 0b1000, 0b0100, 0b0010, 0b1111],
        '3' => [0b0110, 0b1000, 0b0110, 0b1000, 0b0110],
        '4' => [0b1001, 0b1001, 0b1111, 0b1000, 0b1000],
        '5' => [0b1111, 0b0001, 0b0111, 0b1000, 0b0111],
        '6' => [0b0110, 0b0001, 0b0111, 0b1001, 0b0110],
        '7' => [0b1111, 0b1000, 0b0100, 0b0010, 0b0010],
        '8' => [0b0110, 0b1001, 0b0110, 0b1001, 0b0110],
        '9' => [0b0110, 0b1001, 0b1110, 0b1000, 0b0110],
        'A' => [0b0110, 0b1001, 0b1111, 0b1001, 0b1001],
        'B' => [0b0111, 0b1001, 0b0111, 0b1001, 0b0111],
        'C' => [0b0110, 0b0001, 0b0001, 0b0001, 0b0110],
        'D' => [0b0111, 0b1001, 0b1001, 0b1001, 0b0111],
        'E' => [0b1111, 0b0001, 0b0111, 0b0001, 0b1111],
        'F' => [0b1111, 0b0001, 0b0111, 0b0001, 0b0001],
        'G' => [0b0110, 0b0001, 0b1101, 0b1001, 0b0110],
        'H' => [0b1001, 0b1001, 0b1111, 0b1001, 0b1001],
        'I' => [0b0111, 0b0010, 0b0010, 0b0010, 0b0111],
        'J' => [0b1100, 0b1000, 0b1000, 0b1001, 0b0110],
        'K' => [0b1001, 0b0101, 0b0011, 0b0101, 0b1001],
        'L' => [0b0001, 0b0001, 0b0001, 0b0001, 0b1111],
        'M' => [0b1001, 0b1111, 0b0101, 0b1001, 0b1001],
        'N' => [0b1001, 0b1011, 0b1101, 0b1001, 0b1001],
        'O' => [0b0110, 0b1001, 0b1001, 0b1001, 0b0110],
        'P' => [0b0111, 0b1001, 0b0111, 0b0001, 0b0001],
        'Q' => [0b0110, 0b1001, 0b1001, 0b0101, 0b1110],
        'R' => [0b0111, 0b1001, 0b0111, 0b0101, 0b1001],
        'S' => [0b0110, 0b0001, 0b0110, 0b1000, 0b0110],
        'T' => [0b1111, 0b0010, 0b0010, 0b0010, 0b0010],
        'U' => [0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
        'V' => [0b1001, 0b1001, 0b1001, 0b0110, 0b0110],
        'W' => [0b1001, 0b1001, 0b0101, 0b1111, 0b1001],
        'X' => [0b1001, 0b1001, 0b0110, 0b1001, 0b1001],
        'Y' => [0b1001, 0b1001, 0b0110, 0b0010, 0b0010],
        'Z' => [0b1111, 0b1000, 0b0100, 0b0010, 0b1111],
        '.' => [0b0000, 0b0000, 0b0000, 0b0000, 0b0010],
        '/' => [0b1000, 0b0100, 0b0110, 0b0010, 0b0001],
        ':' => [0b0000, 0b0010, 0b0000, 0b0010, 0b0000],
        '-' => [0b0000, 0b0000, 0b1111, 0b0000, 0b0000],
        '%' => [0b1001, 0b0100, 0b0110, 0b0010, 0b1001],
        ' ' => [0b0000, 0b0000, 0b0000, 0b0000, 0b0000],
        _ => return None,
    })
}
