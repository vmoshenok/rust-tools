//! Application core: hidden message window, tray icon, global hotkeys and
//! the capture pipeline (screen grab -> selector -> editor).

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::time::SystemTime;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{self, Config};
use crate::keys::KeyCombo;
use crate::util::{self, wide, APP_NAME};
use crate::{capture, selector, tray};

pub const MAIN_CLASS: &str = "RustShot.Main";
/// Posted by a second instance or the keyboard hook to start a capture.
pub const WM_APP_CAPTURE: u32 = WM_APP + 2;
const TRAY_CAPTURE_TIMER: usize = 1;

struct App {
    hwnd: HWND,
    cfg: Config,
    cfg_path: PathBuf,
    cfg_mtime: Option<SystemTime>,
    /// Number of ids registered with RegisterHotKey (ids 1..=n).
    hotkeys_registered: i32,
    /// Combos handled by the low-level keyboard hook instead.
    hook_combos: Vec<KeyCombo>,
    hook: HHOOK,
    /// vk whose key-up must be swallowed (we swallowed its key-down).
    swallowed_vk: Option<u32>,
    last_saved: Option<PathBuf>,
    taskbar_created_msg: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

pub fn main_hwnd() -> HWND {
    with_app(|a| a.hwnd).unwrap_or(null_mut())
}

pub fn notify(title: &str, text: &str, error: bool) {
    let hwnd = main_hwnd();
    if !hwnd.is_null() {
        tray::notify(hwnd, title, text, error);
    }
}

pub fn remember_saved(path: &std::path::Path) {
    with_app(|a| a.last_saved = Some(path.to_path_buf()));
}

fn config() -> Config {
    with_app(|a| a.cfg.clone()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Hotkeys
// ---------------------------------------------------------------------------

unsafe extern "system" fn keyboard_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lp as *const KBDLLHOOKSTRUCT);
        let down = wp as u32 == WM_KEYDOWN || wp as u32 == WM_SYSKEYDOWN;
        let up = wp as u32 == WM_KEYUP || wp as u32 == WM_SYSKEYUP;
        let pressed = |k: VIRTUAL_KEY| GetAsyncKeyState(k as i32) < 0;
        let swallow = with_app(|a| {
            if down {
                let combo = KeyCombo {
                    vk: kb.vkCode as u16,
                    ctrl: pressed(VK_CONTROL),
                    alt: pressed(VK_MENU),
                    shift: pressed(VK_SHIFT),
                    win: pressed(VK_LWIN) || pressed(VK_RWIN),
                };
                if a.hook_combos.contains(&combo) {
                    if a.swallowed_vk != Some(kb.vkCode) {
                        PostMessageW(a.hwnd, WM_APP_CAPTURE, 0, 0);
                    }
                    a.swallowed_vk = Some(kb.vkCode);
                    return true;
                }
            } else if up && a.swallowed_vk == Some(kb.vkCode) {
                a.swallowed_vk = None;
                return true;
            }
            false
        })
        .unwrap_or(false);
        if swallow {
            return 1;
        }
    }
    CallNextHookEx(null_mut(), code, wp, lp)
}

/// (Re)register the capture hotkeys from the config. PrintScreen combos go
/// through a low-level keyboard hook so they win over the Windows Snipping
/// Tool; others use RegisterHotKey and fall back to the hook if taken.
fn register_hotkeys(a: &mut App) -> Vec<String> {
    unsafe {
        for id in 1..=a.hotkeys_registered {
            UnregisterHotKey(a.hwnd, id);
        }
        if !a.hook.is_null() {
            UnhookWindowsHookEx(a.hook);
            a.hook = null_mut();
        }
    }
    a.hotkeys_registered = 0;
    a.hook_combos.clear();

    let mut errors = Vec::new();
    for combo in a.cfg.capture_hotkeys(&mut errors) {
        let use_hook = combo.vk == VK_SNAPSHOT
            || unsafe { RegisterHotKey(a.hwnd, a.hotkeys_registered + 1, combo.hotkey_mods(), combo.vk as u32) } == 0;
        if use_hook {
            a.hook_combos.push(combo);
        } else {
            a.hotkeys_registered += 1;
        }
    }
    if !a.hook_combos.is_empty() {
        a.hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), util::hinstance(), 0) };
        if a.hook.is_null() {
            errors.push("Could not install the keyboard hook for PrintScreen hotkeys.".into());
        }
    }
    errors
}

fn hotkey_hint(cfg: &Config) -> String {
    let mut e = vec![];
    crate::keys::Binding(cfg.capture_hotkeys(&mut e)).label()
}

// ---------------------------------------------------------------------------
// Config reload
// ---------------------------------------------------------------------------

fn reload_config(force: bool) {
    let msgs = with_app(|a| {
        let mtime = config::modified(&a.cfg_path);
        if !force && mtime == a.cfg_mtime {
            return Vec::new();
        }
        let (cfg, err) = config::load(&a.cfg_path);
        a.cfg = cfg;
        a.cfg_mtime = config::modified(&a.cfg_path);
        let mut msgs: Vec<String> = err.into_iter().collect();
        msgs.extend(register_hotkeys(a));
        msgs
    })
    .unwrap_or_default();
    if !msgs.is_empty() {
        notify("Settings problem", &msgs.join("\n"), true);
    } else if force {
        notify("Settings reloaded", &format!("Capture hotkey: {}", hotkey_hint(&config())), false);
    }
}

// ---------------------------------------------------------------------------
// Capture pipeline
// ---------------------------------------------------------------------------

pub fn start_capture() {
    if selector::is_active() {
        return;
    }
    reload_config(false);
    let cfg = config();
    let shot = capture::grab_screen();
    let targets = capture::window_rects(cfg.capture.detect_windows);
    selector::open(shot, targets, cfg);
}

// ---------------------------------------------------------------------------
// Main window
// ---------------------------------------------------------------------------

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let taskbar_created = with_app(|a| a.taskbar_created_msg).unwrap_or(0);
    if msg == taskbar_created && msg != 0 {
        tray::add(hwnd, APP_NAME); // Explorer restarted
        return 0;
    }
    match msg {
        WM_HOTKEY | WM_APP_CAPTURE => {
            start_capture();
            0
        }
        tray::WM_APP_TRAY => {
            match lp as u32 {
                WM_LBUTTONUP => {
                    let delay = config().capture.tray_delay_ms.max(1);
                    SetTimer(hwnd, TRAY_CAPTURE_TIMER, delay, None);
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => tray::show_menu(hwnd, &hotkey_hint(&config())),
                tray::NIN_BALLOONUSERCLICK => {
                    if let Some(p) = with_app(|a| a.last_saved.clone()).flatten() {
                        util::reveal_in_explorer(&p);
                    }
                }
                _ => {}
            }
            0
        }
        WM_TIMER if wp == TRAY_CAPTURE_TIMER => {
            KillTimer(hwnd, TRAY_CAPTURE_TIMER);
            start_capture();
            0
        }
        WM_COMMAND => {
            match util::loword(wp) as usize {
                tray::ID_CAPTURE => {
                    let delay = config().capture.tray_delay_ms.max(1);
                    SetTimer(hwnd, TRAY_CAPTURE_TIMER, delay, None);
                }
                tray::ID_AUTOSTART => {
                    let enable = !tray::autostart_enabled();
                    if let Err(e) = tray::set_autostart(enable) {
                        util::error_box(hwnd, &e);
                    }
                }
                tray::ID_OPEN_CONFIG => {
                    if let Some(p) = with_app(|a| a.cfg_path.clone()) {
                        util::shell_open("notepad.exe", Some(&format!("\"{}\"", p.display())));
                    }
                }
                tray::ID_RELOAD => reload_config(true),
                tray::ID_OPEN_FOLDER => {
                    let dir = PathBuf::from(config().default_location);
                    let _ = std::fs::create_dir_all(&dir);
                    util::shell_open(&dir.display().to_string(), None);
                }
                tray::ID_EXIT => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            with_app(|a| unsafe {
                for id in 1..=a.hotkeys_registered {
                    UnregisterHotKey(hwnd, id);
                }
                if !a.hook.is_null() {
                    UnhookWindowsHookEx(a.hook);
                }
            });
            tray::remove(hwnd);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub fn run() -> i32 {
    unsafe {
        let hinst = util::hinstance();
        let class = wide(MAIN_CLASS);
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinst;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        // A real (never shown) top-level window: message-only windows do not
        // receive the "TaskbarCreated" broadcast.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide(APP_NAME).as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinst,
            null_mut(),
        );
        if hwnd.is_null() {
            return 1;
        }

        let cfg_path = config::config_path();
        let (cfg, cfg_err) = config::load(&cfg_path);
        let tb_msg = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                hwnd,
                cfg_mtime: config::modified(&cfg_path),
                cfg,
                cfg_path,
                hotkeys_registered: 0,
                hook_combos: Vec::new(),
                hook: null_mut(),
                swallowed_vk: None,
                last_saved: None,
                taskbar_created_msg: tb_msg,
            })
        });

        tray::add(hwnd, &format!("{APP_NAME} — {}", hotkey_hint(&config())));
        let mut errors: Vec<String> = cfg_err.into_iter().collect();
        errors.extend(with_app(register_hotkeys).unwrap_or_default());
        if !errors.is_empty() {
            notify("Settings problem", &errors.join("\n"), true);
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        msg.wParam as i32
    }
}
