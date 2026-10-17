#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const APP_NAME: &str = "Keybinder";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const WM_TRAY: u32 = WM_APP + 1;

const ID_EDIT: usize = 1;
const ID_RELOAD: usize = 2;
const ID_STARTUP: usize = 3;
const ID_EXIT: usize = 4;

const DEFAULT_CONFIG: &str = r#"# Keybinder config. Edit, then choose "Reload config" in the tray menu.
#
# Modifiers: Ctrl, Shift, Alt, Win
# Keys:      A-Z, 0-9, F1-F24, Space, Enter, Tab, Esc, Backspace, Insert, Delete,
#            Home, End, PageUp, PageDown, Up, Down, Left, Right, Num0-Num9,
#            or a single punctuation character like ; or /
# Inside double quotes use \n for Enter and \t for Tab.
macros:
  "Ctrl+Shift+P": "some_looong_string"
  "Ctrl+Alt+E": "me@example.com"
"#;

thread_local! {
    // Text for hotkey id N is stored at index N-1.
    static MACROS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn exe_path() -> PathBuf {
    std::env::current_exe().expect("cannot resolve exe path")
}

// Lives next to the exe so it is found regardless of the working directory (autostart runs from System32).
fn config_path() -> PathBuf {
    exe_path().with_file_name("keybinder.yml")
}

fn message_box(text: &str, flags: MESSAGEBOX_STYLE) {
    let (text, title) = (wide(text), wide(APP_NAME));
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), flags) };
}

// ---------- config ----------

fn load_config() -> Result<Vec<(String, String)>, String> {
    use serde_yaml::Value;

    let path = config_path();
    if !path.exists() {
        std::fs::write(&path, DEFAULT_CONFIG).map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let root: Value = serde_yaml::from_str(&text).map_err(|e| format!("Invalid YAML in {}:\n{e}", path.display()))?;

    // Accept either a `macros:` section or a flat top-level mapping.
    let map = root
        .get("macros")
        .unwrap_or(&root)
        .as_mapping()
        .ok_or("Config must contain a 'macros:' mapping")?;

    let mut out = Vec::new();
    for (k, v) in map {
        let combo = k.as_str().ok_or("Hotkeys must be strings")?.to_string();
        let text = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => return Err(format!("Value for '{combo}' must be a string")),
        };
        out.push((combo, text));
    }
    Ok(out)
}

fn parse_hotkey(combo: &str) -> Result<(HOT_KEY_MODIFIERS, u32), String> {
    let combo = combo.trim();
    // "Ctrl++" means Ctrl and the plus key.
    let (mods_part, key) = match combo.rsplit_once('+') {
        Some((m, "")) if m.ends_with('+') => (&m[..m.len() - 1], "+"),
        Some((m, k)) => (m, k),
        None => ("", combo),
    };

    let mut mods = 0;
    for part in mods_part.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        mods |= match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => MOD_CONTROL,
            "shift" => MOD_SHIFT,
            "alt" => MOD_ALT,
            "win" | "super" | "meta" => MOD_WIN,
            _ => return Err(format!("unknown modifier '{part}'")),
        };
    }
    let vk = parse_key(key.trim()).ok_or_else(|| format!("unknown key '{key}'"))?;
    Ok((mods, vk))
}

fn parse_key(key: &str) -> Option<u32> {
    let lower = key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" => VK_SPACE,
        "enter" | "return" => VK_RETURN,
        "tab" => VK_TAB,
        "esc" | "escape" => VK_ESCAPE,
        "backspace" => VK_BACK,
        "insert" | "ins" => VK_INSERT,
        "delete" | "del" => VK_DELETE,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" | "pgup" => VK_PRIOR,
        "pagedown" | "pgdn" => VK_NEXT,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "pause" => VK_PAUSE,
        "printscreen" => VK_SNAPSHOT,
        _ => 0,
    };
    if named != 0 {
        return Some(named as u32);
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(VK_F1 as u32 + n - 1);
        }
    }
    if let Some(n) = lower.strip_prefix("num").and_then(|n| n.parse::<u32>().ok()) {
        if n <= 9 {
            return Some(VK_NUMPAD0 as u32 + n);
        }
    }
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase() as u32);
        }
        // Punctuation: ask the current keyboard layout which key produces it.
        let scan = unsafe { VkKeyScanW(c as u16) };
        if scan != -1 {
            return Some((scan & 0xff) as u32);
        }
    }
    None
}

/// Unregisters all hotkeys, reloads the config and registers the new ones.
fn apply_config(hwnd: HWND) {
    let old_count = MACROS.with(|m| std::mem::take(&mut *m.borrow_mut()).len());
    for id in 1..=old_count as i32 {
        unsafe { UnregisterHotKey(hwnd, id) };
    }

    let entries = match load_config() {
        Ok(entries) => entries,
        Err(e) => {
            update_tray_tip(hwnd, 0);
            message_box(&e, MB_ICONERROR);
            return;
        }
    };

    let mut macros = Vec::new();
    let mut errors = Vec::new();
    for (combo, text) in entries {
        match parse_hotkey(&combo) {
            Err(e) => errors.push(format!("{combo}: {e}")),
            Ok((mods, vk)) => {
                let id = macros.len() as i32 + 1;
                if unsafe { RegisterHotKey(hwnd, id, mods | MOD_NOREPEAT, vk) } == 0 {
                    errors.push(format!("{combo}: already used by another program"));
                } else {
                    macros.push(text);
                }
            }
        }
    }
    let count = macros.len();
    MACROS.with(|m| *m.borrow_mut() = macros);
    update_tray_tip(hwnd, count);

    if !errors.is_empty() {
        message_box(&format!("Some hotkeys were skipped:\n\n{}", errors.join("\n")), MB_ICONWARNING);
    }
}

// ---------- typing ----------

fn type_text(text: &str) {
    // Wait (up to ~2s) for the user to release the hotkey modifiers,
    // otherwise the typed characters would be combined with Ctrl/Alt/etc.
    let modifiers = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN];
    for _ in 0..200 {
        let held = modifiers.iter().any(|&k| unsafe { GetAsyncKeyState(k as i32) } as u16 & 0x8000 != 0);
        if !held {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let mut inputs = Vec::with_capacity(text.len() * 2);
    let mut push = |vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS| {
        for up in [0, KEYEVENTF_KEYUP] {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags | up, time: 0, dwExtraInfo: 0 },
                },
            });
        }
    };
    for c in text.chars() {
        match c {
            '\r' => {}
            '\n' => push(VK_RETURN, 0, 0),
            '\t' => push(VK_TAB, 0, 0),
            _ => {
                let mut buf = [0u16; 2];
                for &unit in c.encode_utf16(&mut buf).iter() {
                    push(0, unit, KEYEVENTF_UNICODE);
                }
            }
        }
    }
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

// ---------- autostart ----------

fn startup_enabled() -> bool {
    let (key, name) = (wide(RUN_KEY), wide(APP_NAME));
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), null_mut())
            == ERROR_SUCCESS
    }
}

fn set_startup(enable: bool) {
    let (key, name) = (wide(RUN_KEY), wide(APP_NAME));
    let result = unsafe {
        if enable {
            let value = wide(&format!("\"{}\"", exe_path().display()));
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr() as *const c_void,
                (value.len() * 2) as u32,
            )
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr())
        }
    };
    if result != ERROR_SUCCESS {
        message_box(&format!("Failed to update startup setting (error {result})"), MB_ICONERROR);
    }
}

// ---------- icon ----------

/// Rounded rect in a 16x16 design grid: (x0, y0, x1, y1, radius, rgb).
type Shape = (f32, f32, f32, f32, f32, [u8; 3]);

// Mid-tone body so it stands out on both light and dark taskbars.
const BODY: [u8; 3] = [0x4A, 0x6F, 0xA5];
const KEY: [u8; 3] = [0xF4, 0xF7, 0xFB];
const ACCENT: [u8; 3] = [0xFF, 0xA9, 0x2E];

const KEYBOARD: &[Shape] = &[
    (0.0, 3.0, 16.0, 14.0, 1.8, BODY),
    // top row
    (1.0, 5.0, 3.0, 7.0, 0.4, ACCENT),
    (4.0, 5.0, 6.0, 7.0, 0.4, KEY),
    (7.0, 5.0, 9.0, 7.0, 0.4, KEY),
    (10.0, 5.0, 12.0, 7.0, 0.4, KEY),
    (13.0, 5.0, 15.0, 7.0, 0.4, KEY),
    // middle row
    (1.0, 8.0, 3.0, 10.0, 0.4, KEY),
    (4.0, 8.0, 6.0, 10.0, 0.4, KEY),
    (7.0, 8.0, 9.0, 10.0, 0.4, KEY),
    (10.0, 8.0, 12.0, 10.0, 0.4, KEY),
    (13.0, 8.0, 15.0, 10.0, 0.4, KEY),
    // bottom row with space bar
    (1.0, 11.0, 3.0, 13.0, 0.4, KEY),
    (4.0, 11.0, 12.0, 13.0, 0.4, KEY),
    (13.0, 11.0, 15.0, 13.0, 0.4, KEY),
];

/// Renders the keyboard as top-down BGRA (straight alpha), 4x4 supersampled.
fn render_icon(size: u32) -> Vec<u8> {
    const SS: u32 = 4;
    // Snap edges to whole pixels so non-integer scales (e.g. 24px at 150%) stay crisp.
    let k = size as f32 / 16.0;
    let shapes: Vec<Shape> = KEYBOARD
        .iter()
        .map(|&(x0, y0, x1, y1, r, rgb)| ((x0 * k).round(), (y0 * k).round(), (x1 * k).round(), (y1 * k).round(), r * k, rgb))
        .collect();
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (mut sum, mut hits) = ([0u32; 3], 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    // Topmost shape containing the sample wins.
                    let hit = shapes.iter().rev().find(|&&(x0, y0, x1, y1, r, _)| {
                        let dx = (x0 + r - x).max(x - (x1 - r)).max(0.0);
                        let dy = (y0 + r - y).max(y - (y1 - r)).max(0.0);
                        x >= x0 && x <= x1 && y >= y0 && y <= y1 && dx * dx + dy * dy <= r * r
                    });
                    if let Some(&(.., rgb)) = hit {
                        for i in 0..3 {
                            sum[i] += rgb[i] as u32;
                        }
                        hits += 1;
                    }
                }
            }
            let avg = |c: u32| if hits == 0 { 0 } else { (c / hits) as u8 };
            out.extend_from_slice(&[avg(sum[2]), avg(sum[1]), avg(sum[0]), (hits * 255 / (SS * SS)) as u8]);
        }
    }
    out
}

fn create_icon() -> HICON {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    let pixels = render_icon(size);

    // Icon resource layout: BITMAPINFOHEADER (double height), bottom-up BGRA, then a 1bpp AND mask.
    let mut res = Vec::new();
    for v in [40u32, size, size * 2] {
        res.extend_from_slice(&v.to_le_bytes());
    }
    res.extend_from_slice(&1u16.to_le_bytes()); // planes
    res.extend_from_slice(&32u16.to_le_bytes()); // bit count
    res.extend_from_slice(&[0u8; 24]); // compression, image size, resolution, colors
    for row in pixels.chunks_exact(size as usize * 4).rev() {
        res.extend_from_slice(row);
    }
    let mask_stride = size.div_ceil(32) * 4;
    res.resize(res.len() + (mask_stride * size) as usize, 0);

    unsafe {
        CreateIconFromResourceEx(res.as_ptr(), res.len() as u32, 1, 0x0003_0000, size as i32, size as i32, LR_DEFAULTCOLOR)
    }
}

// ---------- tray ----------

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

fn add_tray_icon(hwnd: HWND) {
    let mut nid = tray_data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = create_icon();
    set_tip(&mut nid, APP_NAME);
    unsafe {
        Shell_NotifyIconW(NIM_ADD, &nid);
        // The shell keeps its own copy.
        DestroyIcon(nid.hIcon);
    }
}

fn update_tray_tip(hwnd: HWND, count: usize) {
    let mut nid = tray_data(hwnd);
    nid.uFlags = NIF_TIP;
    set_tip(&mut nid, &format!("{APP_NAME} - {count} macro(s)"));
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn set_tip(nid: &mut NOTIFYICONDATAW, tip: &str) {
    for (dst, src) in nid.szTip.iter_mut().take(127).zip(tip.encode_utf16()) {
        *dst = src;
    }
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        let item = |flags: MENU_ITEM_FLAGS, id: usize, text: &str| {
            let text = wide(text);
            AppendMenuW(menu, flags, id, text.as_ptr());
        };
        item(MF_STRING, ID_EDIT, "Edit config");
        item(MF_STRING, ID_RELOAD, "Reload config");
        item(
            MF_STRING | if startup_enabled() { MF_CHECKED } else { MF_UNCHECKED },
            ID_STARTUP,
            "Run at Windows startup",
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        item(MF_STRING, ID_EXIT, "Exit");

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Required so the menu closes when clicking elsewhere.
        SetForegroundWindow(hwnd);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
    }
}

fn edit_config() {
    let path = wide(&config_path().display().to_string());
    let open = wide("open");
    unsafe {
        // Falls back to Notepad when no app is associated with .yml files.
        if ShellExecuteW(null_mut(), open.as_ptr(), path.as_ptr(), null(), null(), SW_SHOWNORMAL as i32) as isize <= 32 {
            let notepad = wide("notepad.exe");
            ShellExecuteW(null_mut(), open.as_ptr(), notepad.as_ptr(), path.as_ptr(), null(), SW_SHOWNORMAL as i32);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let text = MACROS.with(|m| m.borrow().get(wparam.wrapping_sub(1)).cloned());
            if let Some(text) = text {
                std::thread::spawn(move || type_text(&text));
            }
            0
        }
        WM_TRAY => {
            let event = lparam as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_menu(hwnd);
            }
            0
        }
        WM_COMMAND => {
            match wparam & 0xffff {
                ID_EDIT => edit_config(),
                ID_RELOAD => apply_config(hwnd),
                ID_STARTUP => set_startup(!startup_enabled()),
                ID_EXIT => {
                    Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        // Explorer restarted: the tray icon is gone and must be re-added.
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            add_tray_icon(hwnd);
            let count = MACROS.with(|m| m.borrow().len());
            update_tray_tip(hwnd, count);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    unsafe {
        // Lets the tray icon be rendered at the real screen DPI instead of being upscaled blurry.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let mutex_name = wide("Local\\KeybinderSingleInstance");
        CreateMutexW(null(), 0, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            message_box("Keybinder is already running (see the system tray).", MB_ICONINFORMATION);
            return;
        }

        // Keep the autostart entry pointing at the current exe location if it was moved.
        if startup_enabled() {
            set_startup(true);
        }

        let hinstance = GetModuleHandleW(null());
        let class_name = wide("KeybinderWindow");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinstance;
        wc.lpszClassName = class_name.as_ptr();
        RegisterClassW(&wc);

        // Hidden (never shown) top-level window: receives hotkeys, tray events and TaskbarCreated.
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinstance,
            null(),
        );

        TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Ordering::Relaxed);
        add_tray_icon(hwnd);
        apply_config(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
