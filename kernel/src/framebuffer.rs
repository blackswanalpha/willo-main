//! Bitmap framebuffer writer with M10 scrollback.
//!
//! Every printable character is appended to a heap-backed scrollback ring
//! (capacity `SCROLLBACK_LINES`) in addition to being blit to the visible
//! viewport when `scroll_back == 0`. PgUp/PgDn/End from the keyboard module
//! call `scroll_up`/`scroll_down`/`scroll_end`, which freeze the live tail
//! and re-blit a window of past lines from the ring.

use alloc::collections::VecDeque;
use alloc::string::String;
use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use core::{fmt, ptr};
use noto_sans_mono_bitmap::{
    FontWeight, RasterHeight, RasterizedChar, get_raster, get_raster_width,
};

const LINE_SPACING: usize = 2;
const LETTER_SPACING: usize = 0;
const BORDER_PADDING: usize = 1;

const CHAR_RASTER_HEIGHT: RasterHeight = RasterHeight::Size16;
const CHAR_RASTER_WIDTH: usize = get_raster_width(FontWeight::Regular, CHAR_RASTER_HEIGHT);
const BACKUP_CHAR: char = '�';
const FONT_WEIGHT: FontWeight = FontWeight::Regular;

/// Scrollback ring capacity. ≥256 lines satisfies the M10 acceptance
/// criterion; with the heap bumped to 256 KiB this costs at most a few KiB
/// of resident `String` data.
pub const SCROLLBACK_LINES: usize = 256;

/// Hard cap per scrollback line. Prevents a runaway `print!` without a
/// newline from ballooning a single `String`. Long lines wrap into the next
/// line in the ring (matching the visible behaviour).
const MAX_LINE_LEN: usize = 256;

fn raster_for(c: char) -> RasterizedChar {
    get_raster(c, FONT_WEIGHT, CHAR_RASTER_HEIGHT)
        .or_else(|| get_raster(BACKUP_CHAR, FONT_WEIGHT, CHAR_RASTER_HEIGHT))
        .expect("missing backup glyph")
}

const ROW_HEIGHT: usize = CHAR_RASTER_HEIGHT.val() + LINE_SPACING;

pub struct FrameBufferWriter {
    framebuffer: &'static mut [u8],
    info: FrameBufferInfo,
    x_pos: usize,
    y_pos: usize,
    /// Newline-terminated history. Newest line at the back. The line being
    /// composed (no `\n` yet) lives in `current_line`.
    ring: VecDeque<String>,
    current_line: String,
    /// 0 = live tail. Positive offsets freeze the viewport at lines further
    /// back in `ring`.
    scroll_back: usize,
    /// Number of glyph rows that fit in the framebuffer.
    viewport_rows: usize,
}

impl FrameBufferWriter {
    pub fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        let viewport_rows = info.height.saturating_sub(2 * BORDER_PADDING) / ROW_HEIGHT;
        let mut w = Self {
            framebuffer,
            info,
            x_pos: BORDER_PADDING,
            y_pos: BORDER_PADDING,
            ring: VecDeque::with_capacity(SCROLLBACK_LINES),
            current_line: String::with_capacity(MAX_LINE_LEN.min(128)),
            scroll_back: 0,
            viewport_rows,
        };
        w.clear_visible();
        w
    }

    /// Erase the visible framebuffer without touching the ring or cursor.
    fn clear_visible(&mut self) {
        self.framebuffer.fill(0);
    }

    /// Reset the visible viewport AND the cursor — used by the shell `clear`
    /// command and on hard overflow if we ever need it. Keeps the ring intact
    /// so PgUp can still recover the scrolled-out content.
    pub fn clear(&mut self) {
        self.x_pos = BORDER_PADDING;
        self.y_pos = BORDER_PADDING;
        self.framebuffer.fill(0);
    }

    pub fn info(&self) -> FrameBufferInfo {
        self.info
    }

    pub fn viewport_rows(&self) -> usize {
        self.viewport_rows
    }

    pub fn scroll_back(&self) -> usize {
        self.scroll_back
    }

    /// Snapshot of the ring (oldest → newest) for tests.
    pub fn ring_snapshot(&self) -> alloc::vec::Vec<String> {
        self.ring.iter().cloned().collect()
    }

    fn newline(&mut self) {
        // Commit the in-progress line to the ring before advancing the row,
        // so a redraw triggered by overflow draws the just-completed line
        // at the second-to-bottom row and leaves the bottom row blank for
        // the next char.
        self.push_current_to_ring();
        if self.scroll_back == 0 {
            self.advance_visual_row();
        }
    }

    fn carriage_return(&mut self) {
        self.x_pos = BORDER_PADDING;
    }

    fn push_current_to_ring(&mut self) {
        if self.ring.len() == SCROLLBACK_LINES {
            self.ring.pop_front();
        }
        let mut line = String::new();
        core::mem::swap(&mut line, &mut self.current_line);
        self.ring.push_back(line);
    }

    /// Move the cursor to the next visible row. If that overflows the
    /// framebuffer, redraw the most recent `viewport_rows` lines and place
    /// the cursor on the bottom row. Replaces M8's clear-on-overflow.
    fn advance_visual_row(&mut self) {
        self.y_pos += ROW_HEIGHT;
        self.x_pos = BORDER_PADDING;
        if self.y_pos + CHAR_RASTER_HEIGHT.val() + BORDER_PADDING >= self.info.height {
            self.redraw_visible(0);
            self.y_pos = BORDER_PADDING + self.viewport_rows.saturating_sub(1) * ROW_HEIGHT;
            self.x_pos = BORDER_PADDING;
        }
    }

    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.carriage_return(),
            c => {
                // Soft-wrap the ring's current_line at MAX_LINE_LEN. Push
                // before drawing so the redraw (if it fires) doesn't include
                // the character we are about to render.
                if self.current_line.len() >= MAX_LINE_LEN {
                    self.push_current_to_ring();
                    if self.scroll_back == 0 {
                        self.advance_visual_row();
                    }
                }
                if self.scroll_back == 0 {
                    if self.x_pos + CHAR_RASTER_WIDTH >= self.info.width {
                        self.advance_visual_row();
                    }
                    self.write_rendered(raster_for(c));
                }
                self.current_line.push(c);
            }
        }
    }

    fn write_rendered(&mut self, glyph: RasterizedChar) {
        for (y, row) in glyph.raster().iter().enumerate() {
            for (x, byte) in row.iter().enumerate() {
                self.write_pixel(self.x_pos + x, self.y_pos + y, *byte);
            }
        }
        self.x_pos += glyph.width() + LETTER_SPACING;
    }

    fn write_pixel(&mut self, x: usize, y: usize, intensity: u8) {
        let pixel_offset = y * self.info.stride + x;
        let color = match self.info.pixel_format {
            PixelFormat::Rgb => [intensity, intensity, intensity / 2, 0],
            PixelFormat::Bgr => [intensity / 2, intensity, intensity, 0],
            PixelFormat::U8 => [if intensity > 200 { 0xf } else { 0 }, 0, 0, 0],
            other => {
                self.info.pixel_format = PixelFormat::Rgb;
                panic!("pixel format {:?} not supported", other);
            }
        };
        let bpp = self.info.bytes_per_pixel;
        let byte_offset = pixel_offset * bpp;
        self.framebuffer[byte_offset..byte_offset + bpp].copy_from_slice(&color[..bpp]);
        let _ = unsafe { ptr::read_volatile(&self.framebuffer[byte_offset]) };
    }

    /// Re-blit the visible framebuffer from the ring at a given scroll-back
    /// offset (0 = newest viewport, larger values walk further into history).
    fn redraw_visible(&mut self, scroll_back: usize) {
        self.clear_visible();
        let total = self.ring.len() + 1; // +1 for the in-progress current_line
        let max_back = total.saturating_sub(self.viewport_rows);
        let off = scroll_back.min(max_back);
        // First line index in `ring ++ [current_line]` to draw.
        let first = total.saturating_sub(self.viewport_rows + off);
        let last = total - off;
        let mut row = 0usize;
        let mut x = BORDER_PADDING;
        let mut y = BORDER_PADDING;
        for idx in first..last {
            // Clone the line so the iteration doesn't keep an immutable
            // borrow of `self` while `write_pixel` needs a mutable one.
            let line: String = if idx < self.ring.len() {
                self.ring[idx].clone()
            } else {
                self.current_line.clone()
            };
            x = BORDER_PADDING;
            for c in line.chars() {
                if x + CHAR_RASTER_WIDTH >= self.info.width {
                    break;
                }
                let glyph = raster_for(c);
                for (gy, grow) in glyph.raster().iter().enumerate() {
                    for (gx, byte) in grow.iter().enumerate() {
                        self.write_pixel(x + gx, y + gy, *byte);
                    }
                }
                x += glyph.width() + LETTER_SPACING;
            }
            y += ROW_HEIGHT;
            row += 1;
            if row >= self.viewport_rows {
                break;
            }
        }
        // Cursor tracks the newest visible line (bottom row when at live
        // tail). The caller adjusts as needed for ongoing live writes.
        self.x_pos = x;
        self.y_pos = BORDER_PADDING + row.saturating_sub(1) * ROW_HEIGHT;
    }

    /// Scroll back by `n` lines. Clamped to the depth of the ring.
    pub fn scroll_up(&mut self, n: usize) {
        let total = self.ring.len() + 1;
        let max_back = total.saturating_sub(self.viewport_rows);
        let new = (self.scroll_back + n).min(max_back);
        if new == self.scroll_back {
            return;
        }
        self.scroll_back = new;
        self.redraw_visible(self.scroll_back);
    }

    /// Move the viewport `n` lines toward the live tail.
    pub fn scroll_down(&mut self, n: usize) {
        let new = self.scroll_back.saturating_sub(n);
        if new == self.scroll_back {
            return;
        }
        self.scroll_back = new;
        self.redraw_visible(self.scroll_back);
    }

    /// Snap to the live tail (offset 0) and resume normal write flow.
    pub fn scroll_end(&mut self) {
        if self.scroll_back == 0 {
            return;
        }
        self.scroll_back = 0;
        self.redraw_visible(0);
    }
}

unsafe impl Send for FrameBufferWriter {}
unsafe impl Sync for FrameBufferWriter {}

impl fmt::Write for FrameBufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}
