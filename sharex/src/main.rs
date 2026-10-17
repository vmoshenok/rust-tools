//! RustShot — a tiny, fast screenshot + annotation tool for the Windows tray.
//! See README.md for the user flow and REQUIREMENTS.md for the spec.

#![windows_subsystem = "windows"]

mod app;
mod capture;
mod config;
mod editor;
mod gfx;
mod keys;
mod output;
mod selector;
mod tray;
mod util;

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};

use util::wide;

fn main() {
    unsafe {
        // Physical pixels everywhere: captures are 1:1 on every monitor.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Single instance: starting the exe again triggers a capture in the
        // already running instance (handy for shortcuts / mouse buttons).
        let _mutex = CreateMutexW(std::ptr::null(), 1, wide("Local\\RustShot.SingleInstance").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let hwnd = FindWindowW(wide(app::MAIN_CLASS).as_ptr(), std::ptr::null());
            if !hwnd.is_null() {
                PostMessageW(hwnd, app::WM_APP_CAPTURE, 0, 0);
            }
            return;
        }
    }

    let token = gfx::gdiplus_startup();
    let code = app::run();
    gfx::gdiplus_shutdown(token);
    std::process::exit(code);
}
