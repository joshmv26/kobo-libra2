//! Small e-ink friendly games: Sudoku and Tetris.
pub mod sudoku;
pub mod tetris;

pub use self::sudoku::Sudoku;
pub use self::tetris::Tetris;

use std::time::{SystemTime, UNIX_EPOCH};
use crate::color::{Color, BLACK, GRAY08};
use crate::device::CURRENT_DEVICE;
use crate::font::{Fonts, font_from_style, NORMAL_STYLE};
use crate::framebuffer::Framebuffer;
use crate::geom::Rectangle;

/// Tiny xorshift RNG (no extra dependency).
pub struct Rng(u64);

impl Rng {
    pub fn new() -> Rng {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(88172645463325252);
        Rng(n | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// A rectangular button with a label.
#[derive(Clone)]
pub struct Btn {
    pub rect: Rectangle,
    pub label: String,
    pub on: bool,
}

/// Splits a row into `n` equal buttons separated by `gap`.
pub fn split_row(x0: i32, x1: i32, y0: i32, y1: i32, n: usize, gap: i32) -> Vec<Rectangle> {
    let n = n as i32;
    let w = ((x1 - x0) - gap * (n - 1)) / n;
    (0..n).map(|i| {
        let x = x0 + i * (w + gap);
        rect![x, y0, x + w, y1]
    }).collect()
}

/// Draws `text` centred in `rect`. `px` is the approximate text height in pixels.
pub fn draw_text(fb: &mut dyn Framebuffer, fonts: &mut Fonts, text: &str, px: i32, color: Color, rect: &Rectangle) {
    let dpi = CURRENT_DEVICE.dpi;
    // size is in 1/64 pt; px = pt * dpi / 72
    let size = ((px as f32) * 72.0 / dpi as f32 * 64.0).max(64.0) as u32;
    let style = NORMAL_STYLE.with_size(size);
    let font = font_from_style(fonts, &style, dpi);
    let plan = font.plan(text, None, None);
    let x = rect.min.x + (rect.width() as i32 - plan.width) / 2;
    let xh = font.x_heights.0 as i32;
    let y = rect.min.y + (rect.height() as i32 + xh * 7 / 5) / 2;
    font.render(fb, color, &plan, pt!(x, y));
}

/// Draws a bordered button; `on` inverts the colours.
pub fn draw_btn(fb: &mut dyn Framebuffer, fonts: &mut Fonts, b: &Btn, px: i32) {
    let (bg, fg) = if b.on { (BLACK, crate::color::WHITE) } else { (crate::color::WHITE, BLACK) };
    fb.draw_rectangle(&b.rect, BLACK);
    let inner = rect![b.rect.min.x + 3, b.rect.min.y + 3, b.rect.max.x - 3, b.rect.max.y - 3];
    fb.draw_rectangle(&inner, bg);
    draw_text(fb, fonts, &b.label, px, fg, &b.rect);
}

pub const LINE: Color = GRAY08;
