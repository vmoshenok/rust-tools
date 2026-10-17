//! Procedural app icon: a PC monitor with a pencil scribbling on the screen.
//!
//! Shared between `build.rs` (which embeds the icon into the .exe as a
//! Win32 resource) and nothing else at runtime — the app loads the icon
//! from its own resources. Pure Rust, no dependencies, so the build script
//! stays dependency-free.

#![allow(dead_code)]

#[derive(Clone, Copy)]
struct Rgba(f32, f32, f32, f32);

fn hex(c: u32) -> Rgba {
    Rgba(
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
        1.0,
    )
}

fn lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    Rgba(
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
        a.3 + (b.3 - a.3) * t,
    )
}

/// Signed distance to a rounded rectangle (negative inside).
fn sd_round_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5 - r;
    let hy = (y1 - y0) * 0.5 - r;
    let qx = (px - cx).abs() - hx;
    let qy = (py - cy).abs() - hy;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

/// Distance from point to segment.
fn d_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (qx, qy) = (ax + dx * t - px, ay + dy * t - py);
    (qx * qx + qy * qy).sqrt()
}

/// Colour of the icon at unit-square point (x, y), or None for transparent.
fn sample(x: f32, y: f32) -> Option<Rgba> {
    let mut out: Option<Rgba> = None;
    let mut put = |c: Rgba| out = Some(c);

    // --- Monitor -----------------------------------------------------------
    let (mx0, my0, mx1, my1) = (0.04, 0.12, 0.84, 0.70);
    // stand neck + base
    if x > 0.37 && x < 0.51 && y > 0.66 && y < 0.82 {
        put(lerp(hex(0x4A5568), hex(0x2D3748), (y - 0.66) / 0.16));
    }
    if sd_round_rect(x, y, 0.22, 0.80, 0.66, 0.88, 0.035) < 0.0 {
        put(lerp(hex(0x4A5568), hex(0x1F2733), (y - 0.80) / 0.08));
    }
    // bezel
    if sd_round_rect(x, y, mx0, my0, mx1, my1, 0.07) < 0.0 {
        put(lerp(hex(0x3B4659), hex(0x1F2733), (y - my0) / (my1 - my0)));
    }
    // screen with blue gradient + soft diagonal sheen
    let inset = 0.05;
    if sd_round_rect(x, y, mx0 + inset, my0 + inset, mx1 - inset, my1 - inset, 0.03) < 0.0 {
        let t = (y - my0) / (my1 - my0);
        let mut c = lerp(hex(0x5BC0FF), hex(0x1D4ED8), t);
        if x + y * 0.6 < 0.45 {
            c = lerp(c, hex(0xFFFFFF), 0.12);
        }
        put(c);
    }
    // scribble the pencil left on the screen
    let scribble = [(0.17, 0.48), (0.24, 0.30), (0.31, 0.50), (0.38, 0.33), (0.47, 0.52)];
    for w in scribble.windows(2) {
        if d_segment(x, y, w[0].0, w[0].1, w[1].0, w[1].1) < 0.03 {
            put(hex(0xFFFFFF));
        }
    }

    // --- Pencil ------------------------------------------------------------
    // Axis runs from the tip (on the screen) up and to the right.
    let (tx, ty) = (0.47, 0.53);
    let (ax, ay) = (std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2);
    let (dx, dy) = (x - tx, y - ty);
    let u = dx * ax + dy * ay; // distance from tip along the pencil
    let v = -dx * ay + dy * ax; // signed distance across the pencil
    let hw = 0.085;
    let cone = 0.15;
    let (body_end, ferrule_end, eraser_end) = (0.56, 0.62, 0.70);
    let outline = 0.022;

    let in_pencil = |o: f32| -> bool {
        if u < -o || u > eraser_end + o {
            return false;
        }
        if u < cone {
            v.abs() <= hw * (u.max(0.0) / cone) + o * 1.4
        } else if u > eraser_end - hw {
            // rounded eraser end
            let cu = eraser_end - hw;
            ((u - cu).powi(2) + v * v).sqrt() <= hw + o
        } else {
            v.abs() <= hw + o
        }
    };

    if in_pencil(outline) {
        put(hex(0x141A24));
    }
    if in_pencil(0.0) {
        let c = if u < 0.055 {
            hex(0x2B2B2B) // graphite
        } else if u < cone {
            hex(0xF4CFA0) // wood
        } else if u < body_end {
            if v > hw * 0.35 {
                hex(0xE09A00)
            } else if v < -hw * 0.35 {
                hex(0xFFDB6E)
            } else {
                hex(0xFFC21A)
            }
        } else if u < ferrule_end {
            if ((u - body_end) / 0.02) as i32 % 2 == 0 {
                hex(0xC9D1DB)
            } else {
                hex(0x98A3B3)
            }
        } else {
            hex(0xFF7A9A) // eraser
        };
        put(c);
    }

    out
}

/// Render the icon at `size`×`size`. Returns straight (non-premultiplied)
/// BGRA rows, top-down.
pub fn render_bgra(size: u32) -> Vec<u8> {
    const SS: u32 = 6; // supersampling per axis
    let mut buf = vec![0u8; (size * size * 4) as usize];
    let pad = if size <= 24 { 0.0 } else { 0.01 };
    for py in 0..size {
        for px in 0..size {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    let x = x * (1.0 + 2.0 * pad) - pad;
                    let y = y * (1.0 + 2.0 * pad) - pad;
                    if let Some(c) = sample(x, y) {
                        r += c.0 * c.3;
                        g += c.1 * c.3;
                        b += c.2 * c.3;
                        a += c.3;
                    }
                }
            }
            let n = (SS * SS) as f32;
            let i = ((py * size + px) * 4) as usize;
            if a > 0.0 {
                buf[i] = (b / a * 255.0).round() as u8;
                buf[i + 1] = (g / a * 255.0).round() as u8;
                buf[i + 2] = (r / a * 255.0).round() as u8;
                buf[i + 3] = (a / n * 255.0).round() as u8;
            }
        }
    }
    buf
}

/// Icon image in the classic "DIB" form used inside .ico files and RT_ICON
/// resources: BITMAPINFOHEADER (double height), bottom-up BGRA, AND mask.
pub fn icon_dib(size: u32) -> Vec<u8> {
    let bgra = render_bgra(size);
    let mut out = Vec::new();
    let push32 = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    let push16 = |o: &mut Vec<u8>, v: u16| o.extend_from_slice(&v.to_le_bytes());
    push32(&mut out, 40); // biSize
    push32(&mut out, size); // biWidth
    push32(&mut out, size * 2); // biHeight (XOR + AND)
    push16(&mut out, 1); // biPlanes
    push16(&mut out, 32); // biBitCount
    push32(&mut out, 0); // biCompression
    push32(&mut out, size * size * 4); // biSizeImage
    push32(&mut out, 0);
    push32(&mut out, 0);
    push32(&mut out, 0);
    push32(&mut out, 0);
    for row in (0..size).rev() {
        let s = (row * size * 4) as usize;
        out.extend_from_slice(&bgra[s..s + (size * 4) as usize]);
    }
    let mask_row = size.div_ceil(32) * 4;
    out.extend(std::iter::repeat(0u8).take((mask_row * size) as usize));
    out
}

pub const SIZES: [u32; 10] = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];

/// A standalone .ico file containing all [`SIZES`].
pub fn ico_file() -> Vec<u8> {
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&s| icon_dib(s)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (i, &s) in SIZES.iter().enumerate() {
        let dim = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(images[i].len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += images[i].len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}
