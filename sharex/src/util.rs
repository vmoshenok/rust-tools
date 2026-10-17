//! Small Win32 helpers shared by all modules.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, POINT, RECT, SYSTEMTIME};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, SW_SHOWNORMAL};

pub const APP_NAME: &str = "RustShot";

/// NUL-terminated UTF-16 string.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copy `s` into a fixed UTF-16 buffer (truncating), always NUL-terminated.
pub fn copy_wide(dst: &mut [u16], s: &str) {
    let mut n = 0;
    for (i, c) in s.encode_utf16().take(dst.len() - 1).enumerate() {
        dst[i] = c;
        n = i + 1;
    }
    dst[n] = 0;
}

pub fn hinstance() -> HINSTANCE {
    unsafe { GetModuleHandleW(std::ptr::null()) }
}

pub fn exe_path() -> PathBuf {
    let mut buf = vec![0u16; 32768];
    let n = unsafe { GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) };
    PathBuf::from(String::from_utf16_lossy(&buf[..n as usize]))
}

pub fn error_box(hwnd: HWND, text: &str) {
    let t = wide(text);
    let c = wide(APP_NAME);
    unsafe { MessageBoxW(hwnd, t.as_ptr(), c.as_ptr(), MB_OK | MB_ICONERROR | MB_SETFOREGROUND) };
}

/// Open a file/folder/URL with its default handler.
pub fn shell_open(target: &str, args: Option<&str>) {
    let op = wide("open");
    let t = wide(target);
    let a = args.map(wide);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            t.as_ptr(),
            a.as_ref().map_or(std::ptr::null(), |a| a.as_ptr()),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}

/// Show `path` selected in an Explorer window.
pub fn reveal_in_explorer(path: &Path) {
    shell_open("explorer.exe", Some(&format!("/select,\"{}\"", path.display())));
}

pub fn local_time() -> SYSTEMTIME {
    let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    t
}

/// Work area (screen minus taskbar) of the monitor nearest to `pt`.
pub fn work_area_at(pt: POINT) -> RECT {
    unsafe {
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        mi.rcWork
    }
}

#[inline]
pub fn loword(v: usize) -> u16 {
    (v & 0xffff) as u16
}

/// Signed x/y from an LPARAM (GET_X_LPARAM / GET_Y_LPARAM).
#[inline]
pub fn lparam_xy(lp: isize) -> (i32, i32) {
    ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32)
}

#[inline]
pub fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

#[inline]
pub fn rect_w(r: &RECT) -> i32 {
    r.right - r.left
}

#[inline]
pub fn rect_h(r: &RECT) -> i32 {
    r.bottom - r.top
}

#[inline]
pub fn rect_contains(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

pub fn rect_intersect(a: &RECT, b: &RECT) -> Option<RECT> {
    let r = rect(a.left.max(b.left), a.top.max(b.top), a.right.min(b.right), a.bottom.min(b.bottom));
    (r.left < r.right && r.top < r.bottom).then_some(r)
}

pub fn rect_union(a: &RECT, b: &RECT) -> RECT {
    rect(a.left.min(b.left), a.top.min(b.top), a.right.max(b.right), a.bottom.max(b.bottom))
}

pub fn rect_inflate(r: &RECT, d: i32) -> RECT {
    rect(r.left - d, r.top - d, r.right + d, r.bottom + d)
}
