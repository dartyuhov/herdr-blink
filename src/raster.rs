//! Tiny RGBA helpers: PNG decode, box-filter downscale, alpha compositing.

use std::{fs::File, io::BufReader, path::Path};

#[derive(Debug, Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    pub fn new(width: u32, height: u32) -> Rgba {
        Rgba {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }
}

pub fn decode_png(path: &Path) -> Option<Rgba> {
    let mut decoder = png::Decoder::new(BufReader::new(File::open(path).ok()?));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(Rgba {
        width: info.width,
        height: info.height,
        pixels,
    })
}

/// Area-averaging downscale in premultiplied alpha (no dark fringes).
pub fn resize(src: &Rgba, width: u32, height: u32) -> Rgba {
    let mut out = Rgba::new(width, height);
    if width == 0 || height == 0 || src.width == 0 || src.height == 0 {
        return out;
    }
    let sx = src.width as f32 / width as f32;
    let sy = src.height as f32 / height as f32;
    for oy in 0..height {
        let y0 = oy as f32 * sy;
        let y1 = y0 + sy;
        for ox in 0..width {
            let x0 = ox as f32 * sx;
            let x1 = x0 + sx;
            let mut acc = [0f32; 4];
            let mut total = 0f32;
            let mut y = y0.floor() as u32;
            while (y as f32) < y1 && y < src.height {
                let wy = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).max(0.0);
                let mut x = x0.floor() as u32;
                while (x as f32) < x1 && x < src.width {
                    let wx = (x1.min(x as f32 + 1.0) - x0.max(x as f32)).max(0.0);
                    let w = wx * wy;
                    let i = ((y * src.width + x) * 4) as usize;
                    let a = src.pixels[i + 3] as f32 / 255.0;
                    acc[0] += src.pixels[i] as f32 * a * w;
                    acc[1] += src.pixels[i + 1] as f32 * a * w;
                    acc[2] += src.pixels[i + 2] as f32 * a * w;
                    acc[3] += a * w;
                    total += w;
                    x += 1;
                }
                y += 1;
            }
            let o = ((oy * width + ox) * 4) as usize;
            if total > 0.0 && acc[3] > 0.0 {
                out.pixels[o] = (acc[0] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out.pixels[o + 1] = (acc[1] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out.pixels[o + 2] = (acc[2] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out.pixels[o + 3] = (acc[3] / total * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Copies `src` into `dst` at (`x`, `y`); the strip starts transparent, so a
/// plain copy is enough.
pub fn blit(dst: &mut Rgba, src: &Rgba, x: u32, y: u32) {
    for row in 0..src.height {
        let dy = y + row;
        if dy >= dst.height {
            break;
        }
        let cols = src.width.min(dst.width.saturating_sub(x));
        let s = (row * src.width * 4) as usize;
        let d = ((dy * dst.width + x) * 4) as usize;
        let n = (cols * 4) as usize;
        dst.pixels[d..d + n].copy_from_slice(&src.pixels[s..s + n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_averages_and_keeps_alpha() {
        let mut src = Rgba::new(2, 2);
        // One opaque white pixel, three transparent.
        src.pixels[0..4].copy_from_slice(&[255, 255, 255, 255]);
        let out = resize(&src, 1, 1);
        assert_eq!(&out.pixels, &[255, 255, 255, 64]);
    }

    #[test]
    fn blit_clips_to_destination() {
        let mut dst = Rgba::new(3, 3);
        let mut src = Rgba::new(2, 2);
        src.pixels.fill(9);
        blit(&mut dst, &src, 2, 2);
        assert_eq!(&dst.pixels[(8 * 4)..(9 * 4)], &[9, 9, 9, 9]);
        assert_eq!(dst.pixels.iter().filter(|&&b| b == 9).count(), 4);
    }
}
