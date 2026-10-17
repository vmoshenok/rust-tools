//! Grabbing the whole virtual screen and listing window rectangles for
//! click-to-select.

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::Dib;
use crate::util::{rect, rect_intersect};

/// A frozen copy of the full virtual desktop (all monitors).
pub struct Screenshot {
    /// Virtual-screen origin (may be negative with monitors left/above primary).
    pub x: i32,
    pub y: i32,
    pub img: Dib,
}

pub fn virtual_screen() -> RECT {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        rect(x, y, x + w, y + h)
    }
}

pub fn grab_screen() -> Screenshot {
    let vs = virtual_screen();
    let (w, h) = (vs.right - vs.left, vs.bottom - vs.top);
    let img = Dib::new(w, h);
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        BitBlt(img.dc, 0, 0, w, h, screen, vs.left, vs.top, SRCCOPY | CAPTUREBLT);
        ReleaseDC(std::ptr::null_mut(), screen);
        GdiFlush();
    }
    for px in img.pixels_mut().chunks_exact_mut(4) {
        px[3] = 255;
    }
    Screenshot { x: vs.left, y: vs.top, img }
}

impl Screenshot {
    /// Darkened copy used as the overlay background.
    pub fn dimmed(&self, opacity: f32) -> Dib {
        let d = Dib::new(self.img.w, self.img.h);
        let k = ((1.0 - opacity.clamp(0.0, 1.0)) * 256.0) as u32;
        let src = self.img.pixels();
        let dst = d.pixels_mut();
        for (s, o) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            o[0] = ((s[0] as u32 * k) >> 8) as u8;
            o[1] = ((s[1] as u32 * k) >> 8) as u8;
            o[2] = ((s[2] as u32 * k) >> 8) as u8;
            o[3] = 255;
        }
        d
    }

    /// Copy of the area `r` (in screenshot-local coordinates) as BGRA.
    pub fn crop(&self, r: &RECT) -> (i32, i32, Vec<u8>) {
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let stride = (self.img.w * 4) as usize;
        let src = self.img.pixels();
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in r.top..r.bottom {
            let s = y as usize * stride + r.left as usize * 4;
            out.extend_from_slice(&src[s..s + w as usize * 4]);
        }
        (w, h, out)
    }
}

struct EnumCtx {
    out: Vec<RECT>,
    screen: RECT,
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lp: LPARAM) -> BOOL {
    let ctx = &mut *(lp as *mut EnumCtx);
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
        return 1;
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TRANSPARENT != 0 {
        return 1;
    }
    let mut cloaked: u32 = 0;
    DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut cloaked as *mut _ as *mut _, 4);
    if cloaked != 0 {
        return 1;
    }
    let class = class_name(hwnd);
    if matches!(class.as_str(), "Progman" | "WorkerW") {
        return 1; // desktop: monitors are used as the fallback instead
    }
    let titled = GetWindowTextLengthW(hwnd) > 0;
    let useful_untitled = matches!(
        class.as_str(),
        "#32768" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "tooltips_class32"
    );
    if !titled && !useful_untitled {
        return 1;
    }
    let mut r: RECT = std::mem::zeroed();
    let ok = DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS as u32,
        &mut r as *mut _ as *mut _,
        std::mem::size_of::<RECT>() as u32,
    ) == 0;
    if !ok {
        GetWindowRect(hwnd, &mut r);
    }
    if let Some(r) = rect_intersect(&r, &ctx.screen) {
        if r.right - r.left > 4 && r.bottom - r.top > 4 {
            ctx.out.push(r);
        }
    }
    1
}

unsafe extern "system" fn monitor_proc(_m: HMONITOR, _dc: HDC, r: *mut RECT, lp: LPARAM) -> BOOL {
    let ctx = &mut *(lp as *mut EnumCtx);
    ctx.out.push(*r);
    1
}

/// Window rectangles (virtual-screen coordinates) in Z order, top-most first,
/// followed by monitor rectangles as a fallback for "click on the desktop".
pub fn window_rects(include_windows: bool) -> Vec<RECT> {
    let mut ctx = EnumCtx { out: Vec::new(), screen: virtual_screen() };
    unsafe {
        if include_windows {
            EnumWindows(Some(enum_proc), &mut ctx as *mut _ as LPARAM);
        }
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(monitor_proc), &mut ctx as *mut _ as LPARAM);
    }
    ctx.out
}
