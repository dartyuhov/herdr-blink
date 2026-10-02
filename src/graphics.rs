//! Harness logos over the logo column.
//!
//! herdr caps graphics at 16 layers per pane, so the visible logos are
//! composited into one RGBA strip per logo column. Tree rows sit at up to
//! three indents, so there are at most three strips. They are re-uploaded
//! whenever the visible slots change (scroll / filter / query / view).

use std::{
    collections::{BTreeMap, HashMap, HashSet},
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

    /// Composites one strip: `LOGO_COLS` cells wide, from row `top` down to
    /// the lowest slot. Each logo is centered in its row's logo box.
    pub fn strip(&mut self, top: u16, slots: &[LogoSlot]) -> Option<(Rgba, u16)> {
        let rows = slots.iter().map(|s| s.y + 1 - top).max()?;
        let box_w = self.cell_w * LOGO_COLS as u32;
        let box_h = self.cell_h;
        let mut strip = Rgba::new(box_w, box_h * rows as u32);
        let mut any = false;
        for slot in slots {
            let Some(logo) = self.get(slot.harness) else {
                continue;
            };
            let x = (box_w - logo.width.min(box_w)) / 2;
            let y = (slot.y - top) as u32 * box_h + (box_h - logo.height.min(box_h)) / 2;
            raster::blit(&mut strip, logo, x, y);
            any = true;
        }
        any.then_some((strip, rows))
    }
}

/// Base image id for the strips inside the popup's terminal; each column's
/// strip gets `IMAGE_ID + x`. herdr remaps ids when it forwards placements
/// to the outer terminal, so any constants work.
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
    /// Image ids currently uploaded.
    uploaded: Vec<u32>,
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
            uploaded: Vec::new(),
        })
    }

    /// Harnesses drawn as images rather than fallback glyphs.
    pub fn available(&self) -> &HashSet<Harness> {
        &self.available
    }

    /// Replaces the strips so they cover `slots`: one strip per distinct
    /// column, anchored at that column's topmost slot.
    pub fn draw(&mut self, out: &mut impl Write, slots: &[LogoSlot]) {
        let mut buf = Vec::new();
        for id in self.uploaded.drain(..) {
            delete(&mut buf, id);
        }
        for (x, column) in columns(slots) {
            let top = column.iter().map(|s| s.y).min().unwrap_or_default();
            let Some((strip, rows)) = self.logos.strip(top, &column) else {
                continue;
            };
            let id = IMAGE_ID + u32::from(x);
            // CUP is 1-based; C=1 keeps the cursor where it is.
            let _ = write!(buf, "\x1b7\x1b[{};{}H", top + 1, x + 1);
            transmit(&mut buf, id, &strip, LOGO_COLS, rows);
            buf.extend_from_slice(b"\x1b8");
            self.uploaded.push(id);
        }
        let _ = out.write_all(&buf);
        let _ = out.flush();
    }

    pub fn clear(&mut self, out: &mut impl Write) {
        if !self.uploaded.is_empty() {
            let mut buf = Vec::new();
            for id in self.uploaded.drain(..) {
                delete(&mut buf, id);
            }
            let _ = out.write_all(&buf);
            let _ = out.flush();
        }
    }
}

/// Slots grouped by column `x`, left to right.
fn columns(slots: &[LogoSlot]) -> BTreeMap<u16, Vec<LogoSlot>> {
    let mut out: BTreeMap<u16, Vec<LogoSlot>> = BTreeMap::new();
    for slot in slots {
        out.entry(slot.x).or_default().push(*slot);
    }
    out
}

fn delete(buf: &mut Vec<u8>, id: u32) {
    let _ = write!(buf, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\");
}

fn transmit(buf: &mut Vec<u8>, id: u32, image: &Rgba, cols: u16, rows: u16) {
    let data = BASE64.encode(&image.pixels);
    let mut chunks = data.as_bytes().chunks(CHUNK).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        if first {
            let _ = write!(
                buf,
                "\x1b_Ga=T,f=32,s={},v={},i={id},c={cols},r={rows},C=1,q=2,m={more};",
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

    fn slot(x: u16, y: u16) -> LogoSlot {
        LogoSlot {
            x,
            y,
            harness: Harness::Claude,
        }
    }

    #[test]
    fn strip_skips_missing_logos() {
        let mut logos = Logos::new(PathBuf::from("/nonexistent"), 10, 20);
        assert!(logos.strip(0, &[slot(1, 0)]).is_none());
    }

    #[test]
    fn one_strip_per_column() {
        let dir = std::env::temp_dir().join(format!("blink-logos-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join("claude.png")).unwrap();
        let mut encoder = png::Encoder::new(file, 4, 4);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 4 * 4 * 4]).unwrap();
        writer.finish().unwrap();

        let mut graphics = Graphics {
            logos: Logos::new(dir.clone(), 10, 20),
            available: HashSet::from([Harness::Claude]),
            uploaded: Vec::new(),
        };
        let mut out = Vec::new();
        graphics.draw(&mut out, &[slot(1, 3), slot(5, 4), slot(1, 6), slot(3, 5)]);
        let out = String::from_utf8_lossy(&out);
        assert_eq!(out.matches("a=T").count(), 3);
        for (x, top, rows) in [(1, 3, 4), (3, 5, 1), (5, 4, 1)] {
            let id = IMAGE_ID + x as u32;
            assert!(out.contains(&format!("\x1b[{};{}H", top + 1, x + 1)));
            assert!(out.contains(&format!("i={id},c={LOGO_COLS},r={rows},")));
        }
        assert_eq!(graphics.uploaded.len(), 3);

        let mut out = Vec::new();
        graphics.draw(&mut out, &[slot(1, 3)]);
        let out = String::from_utf8_lossy(&out);
        assert_eq!(out.matches("a=d").count(), 3, "old strips are deleted");
        assert_eq!(out.matches("a=T").count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
