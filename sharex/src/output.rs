//! Final image output: clipboard (CF_DIB + "PNG") and .png files.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::CF_DIB;
use windows_sys::Win32::System::Threading::Sleep;

use crate::util;

pub struct Image {
    pub w: i32,
    pub h: i32,
    /// Top-down BGRA, alpha = 255.
    pub bgra: Vec<u8>,
}

pub fn encode_png(img: &Image, fast: bool) -> Result<Vec<u8>, String> {
    let mut rgb = Vec::with_capacity((img.w * img.h * 3) as usize);
    for px in img.bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.w as u32, img.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(if fast { png::Compression::Fast } else { png::Compression::Default });
        enc.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&rgb).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// 24bpp bottom-up DIB (the most widely understood clipboard bitmap).
fn dib24(img: &Image) -> Vec<u8> {
    let stride = ((img.w * 3 + 3) & !3) as usize;
    let mut v = Vec::with_capacity(40 + stride * img.h as usize);
    let hdr = BITMAPINFOHEADER {
        biSize: 40,
        biWidth: img.w,
        biHeight: img.h,
        biPlanes: 1,
        biBitCount: 24,
        biCompression: BI_RGB,
        biSizeImage: (stride * img.h as usize) as u32,
        biXPelsPerMeter: 3780,
        biYPelsPerMeter: 3780,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let hb: [u8; 40] = unsafe { std::mem::transmute(hdr) };
    v.extend_from_slice(&hb);
    for y in (0..img.h as usize).rev() {
        let row = &img.bgra[y * img.w as usize * 4..(y + 1) * img.w as usize * 4];
        let start = v.len();
        for px in row.chunks_exact(4) {
            v.extend_from_slice(&px[..3]);
        }
        v.resize(start + stride, 0);
    }
    v
}

unsafe fn set_clip(format: u32, data: &[u8]) -> bool {
    let h = GlobalAlloc(GMEM_MOVEABLE, data.len());
    if h.is_null() {
        return false;
    }
    let p = GlobalLock(h) as *mut u8;
    std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
    GlobalUnlock(h);
    if SetClipboardData(format, h).is_null() {
        GlobalFree(h);
        return false;
    }
    true
}

pub fn to_clipboard(hwnd: HWND, img: &Image) -> Result<(), String> {
    let dib = dib24(img);
    let png = encode_png(img, true).ok();
    unsafe {
        // Another app may hold the clipboard for a moment; retry briefly.
        let mut opened = false;
        for _ in 0..20 {
            if OpenClipboard(hwnd) != 0 {
                opened = true;
                break;
            }
            Sleep(25);
        }
        if !opened {
            return Err("Clipboard is busy (another application has it open).".into());
        }
        EmptyClipboard();
        let ok = set_clip(CF_DIB as u32, &dib);
        if let Some(png) = png {
            let name = util::wide("PNG");
            set_clip(RegisterClipboardFormatW(name.as_ptr()), &png);
        }
        CloseClipboard();
        if ok {
            Ok(())
        } else {
            Err("Could not place the image on the clipboard.".into())
        }
    }
}

/// Expand `{yyyy}` `{MM}` `{dd}` `{HH}` `{mm}` `{ss}` `{fff}`.
pub fn expand_pattern(pattern: &str) -> String {
    let t = util::local_time();
    let name = pattern
        .replace("{yyyy}", &format!("{:04}", t.wYear))
        .replace("{MM}", &format!("{:02}", t.wMonth))
        .replace("{dd}", &format!("{:02}", t.wDay))
        .replace("{HH}", &format!("{:02}", t.wHour))
        .replace("{mm}", &format!("{:02}", t.wMinute))
        .replace("{ss}", &format!("{:02}", t.wSecond))
        .replace("{fff}", &format!("{:03}", t.wMilliseconds));
    // Strip characters Windows does not allow in file names.
    name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect()
}

/// Unique `<dir>/<pattern>.png` (adds `_2`, `_3`, ... on collision).
pub fn next_file_path(dir: &Path, pattern: &str) -> PathBuf {
    let base = expand_pattern(pattern);
    let base = if base.trim().is_empty() { "Screenshot".to_string() } else { base };
    let mut p = dir.join(format!("{base}.png"));
    let mut i = 2;
    while p.exists() {
        p = dir.join(format!("{base}_{i}.png"));
        i += 1;
    }
    p
}

pub fn save_png(dir: &Path, pattern: &str, img: &Image) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create folder {}: {e}", dir.display()))?;
    let path = next_file_path(dir, pattern);
    let data = encode_png(img, false)?;
    std::fs::write(&path, data).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_sanitized() {
        let s = expand_pattern("a:b/{yyyy}");
        assert!(s.starts_with("a_b_2"));
        assert_eq!(s.len(), "a_b_".len() + 4);
    }

    #[test]
    fn png_roundtrip_header() {
        let img = Image { w: 2, h: 1, bgra: vec![0, 0, 255, 255, 255, 0, 0, 255] };
        let data = encode_png(&img, true).unwrap();
        assert_eq!(&data[1..4], b"PNG");
        assert_eq!(dib24(&img).len(), 40 + 8);
    }
}
