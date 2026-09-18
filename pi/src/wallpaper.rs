use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use i_slint_core::graphics::{Image, Rgba8Pixel, SharedPixelBuffer};

pub struct Prepared {
    pub sharp: Image,
    pub panel: Image,
    pub menu: Image,
}

pub fn options(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png" || x == "jpg"))
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| (s.to_string(), p.clone()))
        })
        .collect();

    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

pub fn empty_image() -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1))
}

pub fn load(path: &Path) -> Result<Image> {
    Image::load_from_path(path).map_err(|e| anyhow::anyhow!("{}: {e:?}", path.display()))
}

pub fn prepare(path: &Path, width: u32, height: u32) -> Result<Prepared> {
    let source = load(path)?;
    let pixels = source
        .to_rgba8()
        .with_context(|| format!("{} has no readable pixels", path.display()))?;

    let sharp = cover(&pixels, width, height);

    let light = cover(&pixels, width.div_ceil(8).max(1), height.div_ceil(8).max(1));
    let light = upscale(&box_blur(&light, 1), width, height);
    let panel = acrylic(&grade(&light, 1.0, 1.25), [0x0b, 0x0d, 0x12], 0.62, 6);

    let heavy = cover(&pixels, width.div_ceil(16).max(1), height.div_ceil(16).max(1));
    let heavy = upscale(&box_blur(&box_blur(&heavy, 2), 2), width, height);
    let menu = acrylic(&grade(&heavy, 1.0, 1.4), [0x16, 0x16, 0x1b], 0.52, 6);

    Ok(Prepared {
        sharp: Image::from_rgba8(sharp),
        panel: Image::from_rgba8(panel),
        menu: Image::from_rgba8(menu),
    })
}

fn grain(x: u32, y: u32) -> i32 {
    let mut h = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (((h ^ (h >> 16)) & 0xFF) as i32) - 128
}

fn acrylic(
    src: &SharedPixelBuffer<Rgba8Pixel>,
    tint: [u8; 3],
    tint_alpha: f32,
    grain_amp: i32,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let (w, h) = (src.width(), src.height());
    let input = src.as_slice();
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let dst = out.make_mut_slice();

    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let p = &input[i];
            let n = grain(x, y) * grain_amp / 128;

            let blend = |c: u8, t: u8| {
                let mixed = c as f32 * (1.0 - tint_alpha) + t as f32 * tint_alpha;
                (mixed * 0.9 + n as f32).clamp(0.0, 255.0) as u8
            };

            dst[i] = Rgba8Pixel {
                r: blend(p.r, tint[0]),
                g: blend(p.g, tint[1]),
                b: blend(p.b, tint[2]),
                a: 255,
            };
        }
    }
    out
}

pub fn thumbnail(path: &Path, width: u32, height: u32) -> Result<Image> {
    let source = load(path)?;
    let pixels = source
        .to_rgba8()
        .with_context(|| format!("{} has no readable pixels", path.display()))?;
    Ok(Image::from_rgba8(cover(&pixels, width, height)))
}

fn sample(src: &SharedPixelBuffer<Rgba8Pixel>, x: f32, y: f32) -> [f32; 4] {
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    let px = src.as_slice();

    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let at = |ix: i32, iy: i32| -> [f32; 4] {
        let cx = ix.clamp(0, sw - 1) as usize;
        let cy = iy.clamp(0, sh - 1) as usize;
        let p = &px[cy * sw as usize + cx];
        [p.r as f32, p.g as f32, p.b as f32, p.a as f32]
    };

    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    let mut out = [0.0; 4];
    for i in 0..4 {
        let top = a[i] + (b[i] - a[i]) * fx;
        let bottom = c[i] + (d[i] - c[i]) * fx;
        out[i] = top + (bottom - top) * fy;
    }
    out
}

fn cover(
    src: &SharedPixelBuffer<Rgba8Pixel>,
    dw: u32,
    dh: u32,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let (sw, sh) = (src.width() as f32, src.height() as f32);
    let scale = (dw as f32 / sw).max(dh as f32 / sh);
    let off_x = (sw * scale - dw as f32) / 2.0;
    let off_y = (sh * scale - dh as f32) / 2.0;

    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(dw, dh);
    let dst = out.make_mut_slice();

    for y in 0..dh {
        for x in 0..dw {
            let s = sample(src, (x as f32 + off_x) / scale, (y as f32 + off_y) / scale);
            dst[(y * dw + x) as usize] = Rgba8Pixel {
                r: s[0] as u8,
                g: s[1] as u8,
                b: s[2] as u8,
                a: s[3] as u8,
            };
        }
    }
    out
}

fn upscale(
    src: &SharedPixelBuffer<Rgba8Pixel>,
    dw: u32,
    dh: u32,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let sx = src.width() as f32 / dw as f32;
    let sy = src.height() as f32 / dh as f32;

    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(dw, dh);
    let dst = out.make_mut_slice();

    for y in 0..dh {
        for x in 0..dw {
            let s = sample(src, (x as f32 + 0.5) * sx - 0.5, (y as f32 + 0.5) * sy - 0.5);
            dst[(y * dw + x) as usize] = Rgba8Pixel {
                r: s[0] as u8,
                g: s[1] as u8,
                b: s[2] as u8,
                a: s[3] as u8,
            };
        }
    }
    out
}

fn box_blur(src: &SharedPixelBuffer<Rgba8Pixel>, radius: i32) -> SharedPixelBuffer<Rgba8Pixel> {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let input = src.as_slice();
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(src.width(), src.height());
    let dst = out.make_mut_slice();

    for y in 0..h {
        for x in 0..w {
            let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let cx = (x + dx).clamp(0, w - 1) as usize;
                    let cy = (y + dy).clamp(0, h - 1) as usize;
                    let p = &input[cy * w as usize + cx];
                    r += p.r as u32;
                    g += p.g as u32;
                    b += p.b as u32;
                    a += p.a as u32;
                    n += 1;
                }
            }
            dst[(y * w + x) as usize] = Rgba8Pixel {
                r: (r / n) as u8,
                g: (g / n) as u8,
                b: (b / n) as u8,
                a: (a / n) as u8,
            };
        }
    }
    out
}

fn grade(
    src: &SharedPixelBuffer<Rgba8Pixel>,
    gain: f32,
    saturation: f32,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(src.width(), src.height());
    let input = src.as_slice();
    let dst = out.make_mut_slice();

    for (i, p) in input.iter().enumerate() {
        let (r, g, b) = (p.r as f32, p.g as f32, p.b as f32);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let mix = |c: f32| ((luma + (c - luma) * saturation) * gain).clamp(0.0, 255.0) as u8;
        dst[i] = Rgba8Pixel {
            r: mix(r),
            g: mix(g),
            b: mix(b),
            a: p.a,
        };
    }
    out
}
