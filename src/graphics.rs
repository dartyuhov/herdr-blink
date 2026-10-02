//! Harness logos over the logo column.
//!
//! herdr caps graphics at 16 layers per pane, so all visible logos are
//! composited into one RGBA strip covering the logo column, which is
//! re-uploaded whenever the visible rows change (scroll / filter / query).

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

use crate::{
    model::Harness,
    raster::{self, Rgba},
    ui::{LOGO_COLS, LogoSlot},
};

/// Fraction of the cell height a logo occupies.
const LOGO_SCALE: f32 = 0.8;

pub struct Logos {
    dir: PathBuf,
    cell_w: u32,
    cell_h: u32,
    /// Decoded and scaled to one logo box; `None` caches a missing asset.
    scaled: HashMap<Harness, Option<Rgba>>,
}

impl Logos {
    pub fn new(dir: PathBuf, cell_w: u32, cell_h: u32) -> Logos {
        Logos {
            dir,
            cell_w,
            cell_h,
            scaled: HashMap::new(),
        }
    }

    fn get(&mut self, harness: Harness) -> Option<&Rgba> {
        let (dir, w, h) = (&self.dir, self.cell_w, self.cell_h);
        self.scaled
            .entry(harness)
            .or_insert_with(|| {
                let name = harness.logo_name()?;
                let src = raster::decode_png(&dir.join(format!("{name}.png")))?;
                let size = ((h as f32 * LOGO_SCALE).round() as u32).min(w * LOGO_COLS as u32);
                let (sw, sh) = fit(src.width, src.height, size, size);
                Some(raster::resize(&src, sw, sh))
            })
            .as_ref()
    }

    /// Composites one strip: `LOGO_COLS` cells wide, tall enough for the
    /// lowest slot. Each logo is centered in its row's logo box.
    pub fn strip(&mut self, slots: &[LogoSlot]) -> Option<(Rgba, u16)> {
        let rows = slots.iter().map(|s| s.y + 1).max()?;
        let box_w = self.cell_w * LOGO_COLS as u32;
        let box_h = self.cell_h;
        let mut strip = Rgba::new(box_w, box_h * rows as u32);
        let mut any = false;
        for slot in slots {
            let Some(logo) = self.get(slot.harness) else {
                continue;
            };
            let x = (box_w - logo.width.min(box_w)) / 2;
            let y = slot.y as u32 * box_h + (box_h - logo.height.min(box_h)) / 2;
            raster::blit(&mut strip, logo, x, y);
            any = true;
        }
        any.then_some((strip, rows))
    }
}

/// Image id for the strip inside the popup's terminal. herdr remaps ids when
/// it forwards placements to the outer terminal, so any constant works.
const IMAGE_ID: u32 = 7_355_608;
/// Base64 bytes per Kitty APC chunk (protocol maximum is 4096).
const CHUNK: usize = 4096;

/// Logo renderer for the popup.
///
/// A herdr popup has no pane id, so `pane.graphics.set` cannot target it.
/// Instead the popup's own process writes Kitty graphics APC sequences;
/// herdr's per-terminal ghostty-vt parses them and re-renders the placement
/// to the outer terminal, clipped to the popup.
pub struct Graphics {
    logos: Logos,
    available: HashSet<Harness>,
    uploaded: bool,
}

impl Graphics {
    /// Returns `None` (fallback glyphs) when graphics are disabled, the cell
    /// pixel size is unknown, or no logo assets are installed. Only stats
    /// the assets; decoding happens on the first draw, after text is up.
    pub fn init() -> Option<Graphics> {
        if std::env::var_os("BLINK_NO_GRAPHICS").is_some_and(|v| !v.is_empty()) {
            return None;
        }
        let (cell_w, cell_h) = cell_size()?;
        let dir = logo_dir();
        let available: HashSet<Harness> = [
            Harness::Claude,
            Harness::Codex,
            Harness::OpenCode,
            Harness::Pi,
            Harness::Copilot,
        ]
        .into_iter()
        .filter(|h| {
            h.logo_name()
                .is_some_and(|n| dir.join(format!("{n}.png")).is_file())
        })
        .collect();
        (!available.is_empty()).then(|| Graphics {
            logos: Logos::new(dir, cell_w, cell_h),
            available,
            uploaded: false,
        })
    }

    /// Harnesses drawn as images rather than fallback glyphs.
    pub fn available(&self) -> &HashSet<Harness> {
        &self.available
    }

    /// Replaces the strip so it covers `slots`, anchored at the logo
    /// column's top-left cell `origin` (0-based x, y).
    pub fn draw(&mut self, out: &mut impl Write, origin: (u16, u16), slots: &[LogoSlot]) {
        let mut buf = Vec::new();
        if self.uploaded {
            delete(&mut buf);
            self.uploaded = false;
        }
        if let Some((strip, rows)) = self.logos.strip(slots) {
            // CUP is 1-based; C=1 keeps the cursor where it is.
            let _ = write!(buf, "\x1b7\x1b[{};{}H", origin.1 + 1, origin.0 + 1);
            transmit(&mut buf, &strip, LOGO_COLS, rows);
            buf.extend_from_slice(b"\x1b8");
            self.uploaded = true;
        }
        let _ = out.write_all(&buf);
        let _ = out.flush();
    }

    pub fn clear(&mut self, out: &mut impl Write) {
        if self.uploaded {
            let mut buf = Vec::new();
            delete(&mut buf);
            let _ = out.write_all(&buf);
            let _ = out.flush();
            self.uploaded = false;
        }
    }
}

fn delete(buf: &mut Vec<u8>) {
    let _ = write!(buf, "\x1b_Ga=d,d=I,i={IMAGE_ID},q=2\x1b\\");
}

fn transmit(buf: &mut Vec<u8>, image: &Rgba, cols: u16, rows: u16) {
    let data = BASE64.encode(&image.pixels);
    let mut chunks = data.as_bytes().chunks(CHUNK).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        if first {
            let _ = write!(
                buf,
                "\x1b_Ga=T,f=32,s={},v={},i={IMAGE_ID},c={cols},r={rows},C=1,q=2,m={more};",
                image.width, image.height
            );
            first = false;
        } else {
            let _ = write!(buf, "\x1b_Gm={more};");
        }
        buf.extend_from_slice(chunk);
        buf.extend_from_slice(b"\x1b\\");
    }
}

/// Cell size in pixels. herdr's `pane.graphics.info` on the pane under the
/// popup reports the attached client's real cell size; the pty's own
/// TIOCGWINSZ pixels are 0 until herdr's first resize, so they are only a
/// fallback.
fn cell_size() -> Option<(u32, u32)> {
    let from_herdr = context_pane_id().and_then(|pane| {
        let info = crate::herdr::graphics_info(&pane).ok()?;
        let w = info.get("cell_width_px")?.as_u64()? as u32;
        let h = info.get("cell_height_px")?.as_u64()? as u32;
        (w > 0 && h > 0).then_some((w, h))
    });
    from_herdr.or_else(|| {
        let size = crossterm::terminal::window_size().ok()?;
        (size.width > 0 && size.height > 0 && size.columns > 0 && size.rows > 0).then(|| {
            (
                (size.width / size.columns) as u32,
                (size.height / size.rows) as u32,
            )
        })
    })
}

/// The tiled pane under the popup, from `HERDR_PLUGIN_CONTEXT_JSON`.
fn context_pane_id() -> Option<String> {
    let ctx = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").ok()?;
    let ctx: serde_json::Value = serde_json::from_str(&ctx).ok()?;
    Some(ctx.get("focused_pane_id")?.as_str()?.to_string())
}

/// Largest size with `w:h` aspect that fits in `max_w × max_h`.
fn fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (0, 0);
    }
    let scale = (max_w as f32 / w as f32).min(max_h as f32 / h as f32);
    (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    )
}

pub fn logo_dir() -> PathBuf {
    std::env::var_os("HERDR_PLUGIN_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            // Dev fallback: the binary lives in <root>/target/<profile>/.
            let exe = std::env::current_exe().ok()?;
            Some(exe.parent()?.parent()?.parent()?.to_path_buf())
        })
        .unwrap_or_else(|| Path::new(".").to_path_buf())
        .join("assets/logos")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_preserving_aspect() {
        assert_eq!(fit(640, 640, 32, 32), (32, 32));
        assert_eq!(fit(200, 100, 32, 32), (32, 16));
        assert_eq!(fit(0, 10, 32, 32), (0, 0));
    }

    #[test]
    fn strip_skips_missing_logos() {
        let mut logos = Logos::new(PathBuf::from("/nonexistent"), 10, 20);
        let slots = [LogoSlot {
            y: 0,
            harness: Harness::Claude,
        }];
        assert!(logos.strip(&slots).is_none());
    }
}
