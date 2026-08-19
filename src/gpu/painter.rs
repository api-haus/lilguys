//! Immediate-mode 2D primitives. One instanced pipeline draws every shape and every glyph.

use bytemuck::{Pod, Zeroable};
use fontdue::Font;
use std::collections::HashMap;

/// Glyphs are rasterised once at this size and scaled on the GPU. Good enough for graybox HUD
/// text; real speech typography will want per-size rasterisation.
pub const ATLAS_PX: f32 = 48.0;
pub const ATLAS_SIDE: u32 = 1024;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Quad {
    /// centre x, centre y, half-width, half-height — in surface pixels.
    pub xywh: [f32; 4],
    pub color: [f32; 4],
    /// atlas u0,v0,u1,v1. `u0 < 0.0` marks a solid shape with no texture fetch.
    pub uv: [f32; 4],
    pub rot: f32,
    /// Corner radius in pixels. Half the smaller extent gives an ellipse.
    pub radius: f32,
    pub _pad: [f32; 2],
}

const SOLID: [f32; 4] = [-1.0, 0.0, 0.0, 0.0];

pub type Color = [f32; 4];

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}

struct Glyph {
    uv: [f32; 4],
    /// Offsets and size in atlas pixels, relative to the pen position at ATLAS_PX.
    left: f32,
    top: f32,
    w: f32,
    h: f32,
    advance: f32,
}

pub struct Painter {
    pub quads: Vec<Quad>,
    font: Font,
    glyphs: HashMap<char, Glyph>,
    /// Shelf packer cursor.
    pen: (u32, u32, u32),
    pub atlas: Vec<u8>,
    pub atlas_dirty: bool,
}

impl Painter {
    pub fn new(font_bytes: &[u8]) -> anyhow::Result<Self> {
        let font = Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!("font parse: {e}"))?;
        Ok(Self {
            quads: Vec::with_capacity(1024),
            font,
            glyphs: HashMap::new(),
            pen: (1, 1, 0),
            atlas: vec![0; (ATLAS_SIDE * ATLAS_SIDE) as usize],
            atlas_dirty: true,
        })
    }

    pub fn clear(&mut self) {
        self.quads.clear();
    }

    // ---- shapes -----------------------------------------------------------------

    pub fn rrect(&mut self, cx: f32, cy: f32, w: f32, h: f32, radius: f32, color: Color) {
        self.quads.push(Quad {
            xywh: [cx, cy, w * 0.5, h * 0.5],
            color,
            uv: SOLID,
            rot: 0.0,
            radius,
            _pad: [0.0; 2],
        });
    }

    pub fn rrect_rot(
        &mut self, cx: f32, cy: f32, w: f32, h: f32, radius: f32, rot: f32, color: Color,
    ) {
        self.quads.push(Quad {
            xywh: [cx, cy, w * 0.5, h * 0.5],
            color,
            uv: SOLID,
            rot,
            radius,
            _pad: [0.0; 2],
        });
    }

    pub fn rect(&mut self, cx: f32, cy: f32, w: f32, h: f32, color: Color) {
        self.rrect(cx, cy, w, h, 0.0, color);
    }

    pub fn ellipse(&mut self, cx: f32, cy: f32, w: f32, h: f32, color: Color) {
        self.rrect(cx, cy, w, h, w.min(h) * 0.5, color);
    }

    /// Outline drawn as four thin rects, so the SDF stays single-purpose.
    pub fn frame(&mut self, cx: f32, cy: f32, w: f32, h: f32, t: f32, color: Color) {
        let (hw, hh) = (w * 0.5, h * 0.5);
        self.rect(cx, cy - hh + t * 0.5, w, t, color);
        self.rect(cx, cy + hh - t * 0.5, w, t, color);
        self.rect(cx - hw + t * 0.5, cy, t, h - t * 2.0, color);
        self.rect(cx + hw - t * 0.5, cy, t, h - t * 2.0, color);
    }

    /// Segment from a to b, drawn as a rotated capsule.
    pub fn line(&mut self, a: [f32; 2], b: [f32; 2], t: f32, color: Color) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 {
            return;
        }
        self.rrect_rot(
            (a[0] + b[0]) * 0.5,
            (a[1] + b[1]) * 0.5,
            len + t,
            t,
            t * 0.5,
            dy.atan2(dx),
            color,
        );
    }

    // ---- text -------------------------------------------------------------------

    fn glyph(&mut self, c: char) -> &Glyph {
        if !self.glyphs.contains_key(&c) {
            let (m, bitmap) = self.font.rasterize(c, ATLAS_PX);
            let (mut x, mut y, mut shelf) = self.pen;
            if x + m.width as u32 + 1 >= ATLAS_SIDE {
                x = 1;
                y += shelf + 1;
                shelf = 0;
            }
            for row in 0..m.height {
                let dst = ((y + row as u32) * ATLAS_SIDE + x) as usize;
                let src = row * m.width;
                self.atlas[dst..dst + m.width].copy_from_slice(&bitmap[src..src + m.width]);
            }
            self.pen = (x + m.width as u32 + 1, y, shelf.max(m.height as u32));
            self.atlas_dirty = true;
            let s = ATLAS_SIDE as f32;
            self.glyphs.insert(
                c,
                Glyph {
                    uv: [
                        x as f32 / s,
                        y as f32 / s,
                        (x + m.width as u32) as f32 / s,
                        (y + m.height as u32) as f32 / s,
                    ],
                    left: m.xmin as f32,
                    top: -(m.height as f32 + m.ymin as f32),
                    w: m.width as f32,
                    h: m.height as f32,
                    advance: m.advance_width,
                },
            );
        }
        &self.glyphs[&c]
    }

    /// Draws with `x, y` as the left end of the baseline. Returns the advance.
    pub fn text(&mut self, x: f32, y: f32, px: f32, color: Color, s: &str) -> f32 {
        let k = px / ATLAS_PX;
        let mut pen = x;
        for c in s.chars() {
            if c == ' ' {
                pen += self.glyph(' ').advance * k;
                continue;
            }
            let g = self.glyph(c);
            let (uv, left, top, w, h, adv) = (g.uv, g.left, g.top, g.w, g.h, g.advance);
            if w > 0.0 && h > 0.0 {
                self.quads.push(Quad {
                    xywh: [pen + (left + w * 0.5) * k, y + (top + h * 0.5) * k, w * 0.5 * k, h * 0.5 * k],
                    color,
                    uv,
                    rot: 0.0,
                    radius: 0.0,
                    _pad: [0.0; 2],
                });
            }
            pen += adv * k;
        }
        pen - x
    }

    pub fn text_width(&mut self, px: f32, s: &str) -> f32 {
        let k = px / ATLAS_PX;
        s.chars().map(|c| self.glyph(c).advance * k).sum()
    }

    /// Greedy wrap at `max_px`. A single word longer than the line is left to overhang rather
    /// than broken, because a hyphenated thought reads worse than a wide one.
    pub fn wrap(&mut self, px: f32, max_px: f32, text: &str) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            let candidate =
                if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && self.text_width(px, &candidate) > max_px {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }
}
