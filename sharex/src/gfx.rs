//! GDI memory bitmaps and a thin safe-ish wrapper over the GDI+ flat API
//! (anti-aliased lines, paths, text).

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Graphics::GdiPlus::*;

use crate::util::wide;

// ---------------------------------------------------------------------------
// Memory DC + 32bpp top-down DIB section
// ---------------------------------------------------------------------------

pub struct Dib {
    pub dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pub w: i32,
    pub h: i32,
}

impl Dib {
    pub fn new(w: i32, h: i32) -> Dib {
        let (w, h) = (w.max(1), h.max(1));
        unsafe {
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w;
            bi.bmiHeader.biHeight = -h; // top-down
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB;
            let mut bits = null_mut();
            let bmp = CreateDIBSection(null_mut(), &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            assert!(!bmp.is_null(), "CreateDIBSection {w}x{h} failed");
            let dc = CreateCompatibleDC(null_mut());
            let old = SelectObject(dc, bmp);
            Dib { dc, bmp, old, bits: bits as *mut u8, w, h }
        }
    }

    pub fn from_bgra(w: i32, h: i32, px: &[u8]) -> Dib {
        let d = Dib::new(w, h);
        unsafe { GdiFlush() };
        d.pixels_mut().copy_from_slice(&px[..(w * h * 4) as usize]);
        d
    }

    /// BGRA pixels, top-down rows, stride = w*4. Call `GdiFlush` first if GDI
    /// drew into the bitmap.
    pub fn pixels(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.bits, (self.w * self.h * 4) as usize) }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn pixels_mut(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h * 4) as usize) }
    }

    /// Flush GDI and return a copy of the pixels with alpha forced to 255.
    pub fn to_bgra_opaque(&self) -> Vec<u8> {
        unsafe { GdiFlush() };
        let mut v = self.pixels().to_vec();
        for px in v.chunks_exact_mut(4) {
            px[3] = 255;
        }
        v
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp);
            DeleteDC(self.dc);
        }
    }
}

pub fn fill_rect(dc: HDC, r: &RECT, rgb: u32) {
    unsafe {
        let br = CreateSolidBrush(colorref(rgb));
        FillRect(dc, r, br);
        DeleteObject(br);
    }
}

/// 0xRRGGBB -> COLORREF (0x00BBGGRR)
pub fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
}

/// 0xRRGGBB + alpha -> GDI+ ARGB
pub fn argb(rgb: u32, a: u8) -> u32 {
    ((a as u32) << 24) | (rgb & 0xffffff)
}

/// Perceived brightness 0..1 of 0xRRGGBB.
pub fn luminance(rgb: u32) -> f32 {
    let r = ((rgb >> 16) & 0xff) as f32;
    let g = ((rgb >> 8) & 0xff) as f32;
    let b = (rgb & 0xff) as f32;
    (0.299 * r + 0.587 * g + 0.114 * b) / 255.0
}

// ---------------------------------------------------------------------------
// GDI+
// ---------------------------------------------------------------------------

pub fn gdiplus_startup() -> usize {
    let mut token = 0usize;
    let input = GdiplusStartupInput {
        GdiplusVersion: 1,
        DebugEventCallback: 0,
        SuppressBackgroundThread: 0,
        SuppressExternalCodecs: 0,
    };
    unsafe { GdiplusStartup(&mut token, &input, null_mut()) };
    token
}

pub fn gdiplus_shutdown(token: usize) {
    unsafe { GdiplusShutdown(token) };
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct P {
    pub x: f32,
    pub y: f32,
}

impl P {
    pub fn new(x: f32, y: f32) -> P {
        P { x, y }
    }
    pub fn dist(self, o: P) -> f32 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

fn pf(p: P) -> PointF {
    PointF { X: p.x, Y: p.y }
}

pub struct Graphics {
    g: *mut GpGraphics,
}

pub struct Font {
    family: *mut GpFontFamily,
    font: *mut GpFont,
    pub size: f32,
    pub bold: bool,
}

impl Font {
    pub fn new(family: &str, size: f32, bold: bool) -> Option<Font> {
        unsafe {
            let name = wide(family);
            let mut fam = null_mut();
            if GdipCreateFontFamilyFromName(name.as_ptr(), null_mut(), &mut fam) != Ok {
                let fallback = wide("Segoe UI");
                if GdipCreateFontFamilyFromName(fallback.as_ptr(), null_mut(), &mut fam) != Ok {
                    return None;
                }
            }
            let mut font = null_mut();
            let style = if bold { FontStyleBold } else { 0 };
            GdipCreateFont(fam, size, style, UnitPixel, &mut font);
            Some(Font { family: fam, font, size, bold })
        }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_null() {
                GdipDeleteFont(self.font);
            }
            GdipDeleteFontFamily(self.family);
        }
    }
}

struct Pen(*mut GpPen);

impl Pen {
    fn new(color: u32, width: f32, round: bool) -> Pen {
        let mut p = null_mut();
        unsafe {
            GdipCreatePen1(color, width, UnitPixel, &mut p);
            if round {
                GdipSetPenLineJoin(p, LineJoinRound);
                GdipSetPenStartCap(p, LineCapRound);
                GdipSetPenEndCap(p, LineCapRound);
            }
        }
        Pen(p)
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        unsafe { GdipDeletePen(self.0) };
    }
}

struct Brush(*mut GpSolidFill);

impl Brush {
    fn new(color: u32) -> Brush {
        let mut b = null_mut();
        unsafe { GdipCreateSolidFill(color, &mut b) };
        Brush(b)
    }
    fn ptr(&self) -> *mut GpBrush {
        self.0 as *mut GpBrush
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        unsafe { GdipDeleteBrush(self.0 as *mut GpBrush) };
    }
}

struct Path(*mut GpPath);

impl Path {
    fn new() -> Path {
        let mut p = null_mut();
        unsafe { GdipCreatePath(FillModeAlternate, &mut p) };
        Path(p)
    }
}

impl Drop for Path {
    fn drop(&mut self) {
        unsafe { GdipDeletePath(self.0) };
    }
}

struct StringFormat(*mut GpStringFormat);

impl StringFormat {
    /// Typographic layout (no extra padding, so measured widths are exact
    /// advances), no wrapping, no clipping, trailing spaces measured.
    fn new() -> StringFormat {
        let mut f = null_mut();
        unsafe {
            let mut generic = null_mut();
            GdipStringFormatGetGenericTypographic(&mut generic);
            if generic.is_null() || GdipCloneStringFormat(generic, &mut f) != Ok {
                GdipCreateStringFormat(0, 0, &mut f);
            }
            GdipSetStringFormatFlags(f, 0x1000 | 0x0800 | 0x4000);
        }
        StringFormat(f)
    }
}

impl Drop for StringFormat {
    fn drop(&mut self) {
        unsafe { GdipDeleteStringFormat(self.0) };
    }
}

impl Graphics {
    pub fn from_hdc(dc: HDC) -> Graphics {
        let mut g = null_mut();
        unsafe {
            GdipCreateFromHDC(dc, &mut g);
            GdipSetSmoothingMode(g, SmoothingModeAntiAlias8x8);
            GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);
            GdipSetTextRenderingHint(g, TextRenderingHintAntiAlias);
        }
        Graphics { g }
    }

    pub fn translate_scale(&self, dx: f32, dy: f32, s: f32) {
        unsafe {
            GdipResetWorldTransform(self.g);
            GdipTranslateWorldTransform(self.g, dx, dy, MatrixOrderPrepend);
            GdipScaleWorldTransform(self.g, s, s, MatrixOrderPrepend);
        }
    }

    pub fn reset_transform(&self) {
        unsafe { GdipResetWorldTransform(self.g) };
    }

    pub fn clip(&self, x: f32, y: f32, w: f32, h: f32) {
        unsafe { GdipSetClipRect(self.g, x, y, w, h, CombineModeReplace) };
    }

    pub fn reset_clip(&self) {
        unsafe { GdipResetClip(self.g) };
    }

    pub fn line(&self, a: P, b: P, color: u32, width: f32) {
        let pen = Pen::new(color, width, true);
        unsafe { GdipDrawLine(self.g, pen.0, a.x, a.y, b.x, b.y) };
    }

    pub fn polyline(&self, pts: &[P], color: u32, width: f32) {
        if pts.len() == 1 {
            let r = width / 2.0;
            self.fill_ellipse(pts[0].x - r, pts[0].y - r, width, width, color);
            return;
        }
        let v: Vec<PointF> = pts.iter().map(|&p| pf(p)).collect();
        let pen = Pen::new(color, width, true);
        unsafe { GdipDrawLines(self.g, pen.0, v.as_ptr(), v.len() as i32) };
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, color: u32, width: f32) {
        let pen = Pen::new(color, width, false);
        unsafe { GdipDrawRectangle(self.g, pen.0, x, y, w, h) };
    }

    pub fn dashed_rect(&self, x: f32, y: f32, w: f32, h: f32, color: u32, width: f32) {
        let pen = Pen::new(color, width, false);
        unsafe {
            GdipSetPenDashStyle(pen.0, DashStyleDash);
            GdipDrawRectangle(self.g, pen.0, x, y, w, h);
        }
    }

    pub fn fill_rect(&self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        let b = Brush::new(color);
        unsafe { GdipFillRectangle(self.g, b.ptr(), x, y, w, h) };
    }

    pub fn fill_ellipse(&self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        let b = Brush::new(color);
        unsafe { GdipFillEllipse(self.g, b.ptr(), x, y, w, h) };
    }

    pub fn ellipse(&self, x: f32, y: f32, w: f32, h: f32, color: u32, width: f32) {
        let pen = Pen::new(color, width, false);
        unsafe { GdipDrawEllipse(self.g, pen.0, x, y, w, h) };
    }

    pub fn fill_polygon(&self, pts: &[P], color: u32) {
        let v: Vec<PointF> = pts.iter().map(|&p| pf(p)).collect();
        let b = Brush::new(color);
        unsafe { GdipFillPolygon(self.g, b.ptr(), v.as_ptr(), v.len() as i32, FillModeAlternate) };
    }

    fn round_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
        let p = Path::new();
        let d = (r * 2.0).min(w).min(h);
        unsafe {
            GdipAddPathArc(p.0, x, y, d, d, 180.0, 90.0);
            GdipAddPathArc(p.0, x + w - d, y, d, d, 270.0, 90.0);
            GdipAddPathArc(p.0, x + w - d, y + h - d, d, d, 0.0, 90.0);
            GdipAddPathArc(p.0, x, y + h - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(p.0);
        }
        p
    }

    pub fn fill_round_rect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
        let p = Self::round_rect_path(x, y, w, h, r);
        let b = Brush::new(color);
        unsafe { GdipFillPath(self.g, b.ptr(), p.0) };
    }

    pub fn round_rect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32, width: f32) {
        let p = Self::round_rect_path(x, y, w, h, r);
        let pen = Pen::new(color, width, false);
        unsafe { GdipDrawPath(self.g, pen.0, p.0) };
    }

    /// Text as a filled path with an optional outline (halo) for contrast.
    pub fn outlined_text(&self, text: &str, x: f32, y: f32, font: &Font, fill: u32, outline: Option<(u32, f32)>) {
        if text.is_empty() {
            return;
        }
        let w: Vec<u16> = text.encode_utf16().collect();
        let p = Path::new();
        let fmt = StringFormat::new();
        let layout = RectF { X: x, Y: y, Width: 0.0, Height: 0.0 };
        let style = if font.bold { FontStyleBold } else { 0 };
        unsafe {
            GdipAddPathString(p.0, w.as_ptr(), w.len() as i32, font.family, style, font.size, &layout, fmt.0);
            if let Some((c, width)) = outline {
                let pen = Pen::new(c, width, true);
                GdipDrawPath(self.g, pen.0, p.0);
            }
            let b = Brush::new(fill);
            GdipFillPath(self.g, b.ptr(), p.0);
        }
    }

    /// Plain (UI) text.
    pub fn text(&self, text: &str, x: f32, y: f32, font: &Font, color: u32) {
        let w: Vec<u16> = text.encode_utf16().collect();
        let fmt = StringFormat::new();
        let layout = RectF { X: x, Y: y, Width: 0.0, Height: 0.0 };
        let b = Brush::new(color);
        unsafe {
            GdipDrawString(self.g, w.as_ptr(), w.len() as i32, font.font, &layout, fmt.0, b.ptr() as *const GpBrush)
        };
    }

    /// Size of `text` when drawn with `font` (consistent with `text` and
    /// `outlined_text`, including trailing spaces).
    pub fn measure(&self, text: &str, font: &Font) -> (f32, f32) {
        let s = if text.is_empty() { "A" } else { text };
        let w: Vec<u16> = s.encode_utf16().collect();
        let fmt = StringFormat::new();
        let layout = RectF { X: 0.0, Y: 0.0, Width: 0.0, Height: 0.0 };
        let mut bb = RectF { X: 0.0, Y: 0.0, Width: 0.0, Height: 0.0 };
        unsafe {
            GdipMeasureString(self.g, w.as_ptr(), w.len() as i32, font.font, &layout, fmt.0, &mut bb, null_mut(), null_mut())
        };
        (if text.is_empty() { 0.0 } else { bb.Width }, bb.Height)
    }
}

impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe { GdipDeleteGraphics(self.g) };
    }
}
