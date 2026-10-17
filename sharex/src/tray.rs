//! Notification-area (tray) icon, its context menu, and the
//! "Run at Windows startup" registry switch.

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::util::{self, copy_wide, wide, APP_NAME};

pub const WM_APP_TRAY: u32 = WM_APP + 1;
/// Balloon clicked (lParam of the tray callback message).
pub const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;

pub const ID_CAPTURE: usize = 1;
pub const ID_AUTOSTART: usize = 2;
pub const ID_OPEN_CONFIG: usize = 3;
pub const ID_RELOAD: usize = 4;
pub const ID_OPEN_FOLDER: usize = 5;
pub const ID_EXIT: usize = 6;

fn base_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

pub fn app_icon(size: i32) -> HICON {
    unsafe { LoadImageW(util::hinstance(), 1 as _, IMAGE_ICON, size, size, LR_DEFAULTCOLOR) as HICON }
}

pub fn add(hwnd: HWND, tip: &str) {
    let mut nid = base_data(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_APP_TRAY;
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    nid.hIcon = app_icon(size);
    copy_wide(&mut nid.szTip, tip);
    unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
}

pub fn remove(hwnd: HWND) {
    let nid = base_data(hwnd);
    unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
}

/// Balloon / toast notification from the tray icon.
pub fn notify(hwnd: HWND, title: &str, text: &str, error: bool) {
    let mut nid = base_data(hwnd);
    nid.uFlags = NIF_INFO;
    copy_wide(&mut nid.szInfoTitle, title);
    copy_wide(&mut nid.szInfo, text);
    nid.dwInfoFlags = if error { NIIF_ERROR } else { NIIF_USER | NIIF_LARGE_ICON };
    if !error {
        nid.hBalloonIcon = app_icon(unsafe { GetSystemMetrics(SM_CXICON) });
    }
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

pub fn show_menu(hwnd: HWND, capture_hint: &str) {
    unsafe {
        let m = CreatePopupMenu();
        let capture = if capture_hint.is_empty() {
            "Make screen".to_string()
        } else {
            format!("Make screen\t{capture_hint}")
        };
        let items: [(usize, u32, String); 9] = [
            (ID_CAPTURE, MF_STRING, capture),
            (0, MF_SEPARATOR, String::new()),
            (
                ID_AUTOSTART,
                MF_STRING | if autostart_enabled() { MF_CHECKED } else { MF_UNCHECKED },
                "Run at Windows startup".into(),
            ),
            (0, MF_SEPARATOR, String::new()),
            (ID_OPEN_FOLDER, MF_STRING, "Open screenshots folder".into()),
            (ID_OPEN_CONFIG, MF_STRING, "Open settings (config.yml)".into()),
            (ID_RELOAD, MF_STRING, "Reload settings".into()),
            (0, MF_SEPARATOR, String::new()),
            (ID_EXIT, MF_STRING, "Exit".into()),
        ];
        for (id, flags, text) in items {
            let w = wide(&text);
            AppendMenuW(m, flags, id, if flags & MF_SEPARATOR != 0 { std::ptr::null() } else { w.as_ptr() });
        }
        SetMenuDefaultItem(m, ID_CAPTURE as u32, 0);
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Required so the menu closes when the user clicks elsewhere.
        SetForegroundWindow(hwnd);
        TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RIGHTALIGN, pt.x, pt.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(m);
    }
}

// --- Run at startup (HKCU\...\Run) -------------------------------------------

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn run_command() -> String {
    format!("\"{}\"", util::exe_path().display())
}

pub fn autostart_enabled() -> bool {
    let key = wide(RUN_KEY);
    let name = wide(APP_NAME);
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut size,
        )
    };
    if rc != 0 {
        return false;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    let value = String::from_utf16_lossy(&buf[..len]);
    value.trim_matches('"').eq_ignore_ascii_case(run_command().trim_matches('"'))
}

pub fn set_autostart(enable: bool) -> Result<(), String> {
    let key = wide(RUN_KEY);
    let name = wide(APP_NAME);
    let rc = unsafe {
        if enable {
            let cmd = wide(&run_command());
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                cmd.as_ptr() as *const _,
                (cmd.len() * 2) as u32,
            )
        } else {
            let rc = RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
            if rc == 2 { 0 } else { rc } // ERROR_FILE_NOT_FOUND: already off
        }
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("Registry update failed (error {rc})"))
    }
}
