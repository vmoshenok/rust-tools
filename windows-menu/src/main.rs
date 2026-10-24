//! Windows Explorer context-menu viewer/editor: one table of every menu item,
//! where it is registered, and an editor for its caption and command.

#![windows_subsystem = "windows"]

mod reg;
mod scan;

use reg::{from_wide, wide, Hive};
use scan::{Entry, Kind, BLOCKED_KEY, COLUMNS};
use std::ptr::{null, null_mut};
use windows_sys::core::PCWSTR;
use windows_sys::w;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::Registry::REG_SZ;
use windows_sys::Win32::System::SystemServices::{SS_CENTERIMAGE, SS_ENDELLIPSIS};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus};
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const ID_FILTER: i32 = 100;
const ID_FILE_TYPES: i32 = 101;
const ID_REFRESH: i32 = 102;
const ID_LIST: i32 = 103;
const ID_CAPTION: i32 = 104;
const ID_COMMAND: i32 = 105;
const ID_SAVE: i32 = 107;
const ID_TOGGLE: i32 = 108;
const ID_OPEN: i32 = 109;

/// Posted (not sent) so selection handling never runs nested inside a ListView call.
const WM_APP_SELCHANGED: u32 = WM_APP + 1;
const WM_APP_SCAN: u32 = WM_APP + 2;

struct App {
    hwnd: HWND,
    font: HFONT,
    scale: f32,
    list: HWND,
    filter_label: HWND,
    filter: HWND,
    file_types: HWND,
    refresh: HWND,
    count: HWND,
    labels: [HWND; 4],
    caption: HWND,
    command: HWND,
    location: HWND,
    info: HWND,
    save: HWND,
    toggle: HWND,
    open: HWND,
    entries: Vec<Entry>,
    view: Vec<usize>,
    selected: Option<usize>,
    sort: Option<(usize, bool)>,
    is_admin: bool,
}

fn get_text(h: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(h);
        let mut buf = vec![0u16; len as usize + 1];
        GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        from_wide(&buf)
    }
}

fn set_text(h: HWND, s: &str) {
    let w = wide(s);
    unsafe { SetWindowTextW(h, w.as_ptr()) };
}

fn error_text(code: u32) -> String {
    match code {
        ERROR_ACCESS_DENIED => "Access denied".into(),
        _ => format!("error {code}"),
    }
}

impl App {
    fn s(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }

    unsafe fn child(&self, class: PCWSTR, text: &str, style: u32, ex: u32, id: i32) -> HWND {
        let t = wide(text);
        let h = CreateWindowExW(
            ex,
            class,
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            self.hwnd,
            id as isize as HMENU,
            GetModuleHandleW(null()),
            null(),
        );
        SendMessageW(h, WM_SETFONT, self.font as WPARAM, 1);
        h
    }

    unsafe fn create(&mut self) {
        let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
        ncm.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
        SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut _, 0);
        self.font = CreateFontIndirectW(&ncm.lfMessageFont);

        let label = WS_CHILD | SS_CENTERIMAGE as u32;
        let edit = WS_TABSTOP | ES_AUTOHSCROLL as u32;
        let button = WS_TABSTOP | BS_PUSHBUTTON as u32;
        self.filter_label = self.child(w!("STATIC"), "Filter:", label, 0, 0);
        self.filter = self.child(w!("EDIT"), "", edit, WS_EX_CLIENTEDGE, ID_FILTER);
        self.file_types = self.child(
            w!("BUTTON"),
            "Include per-file-type entries (.txt, .jpg, ...)",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            ID_FILE_TYPES,
        );
        self.refresh = self.child(w!("BUTTON"), "Refresh", button, 0, ID_REFRESH);
        self.count = self.child(w!("STATIC"), "Scanning...", label, 0, 0);

        let style = WS_TABSTOP | (LVS_REPORT | LVS_OWNERDATA | LVS_SHOWSELALWAYS | LVS_SINGLESEL) as u32;
        self.list = self.child(WC_LISTVIEWW, "", style, WS_EX_CLIENTEDGE, ID_LIST);
        let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP;
        SendMessageW(self.list, LVM_SETEXTENDEDLISTVIEWSTYLE, ex as WPARAM, ex as LPARAM);
        for (i, (title, width)) in COLUMNS.iter().enumerate() {
            let mut t = wide(title);
            let col = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
                cx: self.s(*width),
                pszText: t.as_mut_ptr(),
                iSubItem: i as i32,
                ..std::mem::zeroed()
            };
            SendMessageW(self.list, LVM_INSERTCOLUMNW, i, &col as *const _ as LPARAM);
        }

        for (i, text) in ["Caption:", "Command:", "Location:", "Note:"].iter().enumerate() {
            self.labels[i] = self.child(w!("STATIC"), text, label, 0, 0);
        }
        self.caption = self.child(w!("EDIT"), "", edit, WS_EX_CLIENTEDGE, ID_CAPTION);
        self.command = self.child(w!("EDIT"), "", edit, WS_EX_CLIENTEDGE, ID_COMMAND);
        self.location = self.child(w!("EDIT"), "", edit | ES_READONLY as u32, WS_EX_CLIENTEDGE, 0);
        self.info = self.child(w!("STATIC"), "", label | SS_ENDELLIPSIS as u32, 0, 0);
        self.save = self.child(w!("BUTTON"), "Save", button, 0, ID_SAVE);
        self.toggle = self.child(w!("BUTTON"), "Disable", button, 0, ID_TOGGLE);
        self.open = self.child(w!("BUTTON"), "Open in Regedit", button, 0, ID_OPEN);
        self.show_detail();
        SetFocus(self.filter);
    }

    unsafe fn layout(&self) {
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(self.hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        let m = self.s(8);
        let rh = self.s(24);
        let gap = self.s(6);
        let place = |hw: HWND, x: i32, y: i32, cx: i32, cy: i32| {
            MoveWindow(hw, x, y, cx.max(0), cy.max(0), 1);
        };

        let mut x = m;
        place(self.filter_label, x, m, self.s(40), rh);
        x += self.s(42);
        place(self.filter, x, m, self.s(260), rh);
        x += self.s(260) + 2 * gap;
        place(self.file_types, x, m, self.s(290), rh);
        x += self.s(290) + gap;
        place(self.refresh, x, m, self.s(80), rh);
        x += self.s(80) + 2 * gap;
        place(self.count, x, m, w - x - m, rh);

        let rows = 5;
        let bottom = rows * rh + (rows - 1) * gap;
        let by = h - m - bottom;
        let ly = m + rh + gap;
        place(self.list, m, ly, w - 2 * m, by - gap - ly);

        let lw = self.s(70);
        let ex = m + lw;
        let ew = w - ex - m;
        let row = |i: i32| by + i * (rh + gap);
        for (i, l) in self.labels.iter().enumerate() {
            place(*l, m, row(i as i32), lw, rh);
        }
        place(self.caption, ex, row(0), ew, rh);
        place(self.command, ex, row(1), ew, rh);
        place(self.location, ex, row(2), ew, rh);
        place(self.info, ex, row(3), ew, rh);
        let mut bx = ex;
        for (b, bw) in [(self.save, 90), (self.toggle, 110), (self.open, 130)] {
            place(b, bx, row(4), self.s(bw), rh);
            bx += self.s(bw) + gap;
        }
    }

    /// Rescans everything, keeping the selected entry selected if it still exists.
    unsafe fn rescan(&mut self) {
        let keep = self.selected.map(|i| {
            let e = &self.entries[i];
            (e.location(), e.scope.clone(), e.caption.clone())
        });
        let old_cursor = SetCursor(LoadCursorW(null_mut(), IDC_WAIT));
        let file_types = SendMessageW(self.file_types, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
        self.entries = scan::scan(file_types);
        SetCursor(old_cursor);
        self.selected = keep.and_then(|(loc, scope, caption)| {
            let same = |e: &Entry| e.location() == loc && e.scope == scope;
            let idx = self.entries.iter().position(|e| same(e) && e.caption == caption);
            idx.or_else(|| self.entries.iter().position(same))
        });
        self.rebuild_view();
    }

    unsafe fn rebuild_view(&mut self) {
        let filter = get_text(self.filter).to_lowercase();
        let entries = &self.entries;
        self.view = (0..entries.len())
            .filter(|&i| {
                filter.is_empty() || (0..COLUMNS.len()).any(|c| entries[i].column(c).to_lowercase().contains(&filter))
            })
            .collect();
        if let Some((col, asc)) = self.sort {
            self.view.sort_by_cached_key(|&i| entries[i].column(col).to_lowercase());
            if !asc {
                self.view.reverse();
            }
        }
        SendMessageW(self.list, LVM_SETITEMCOUNT, self.view.len(), 0);
        InvalidateRect(self.list, null(), 1);
        set_text(self.count, &format!("{} of {} items", self.view.len(), self.entries.len()));

        let mut item: LVITEMW = std::mem::zeroed();
        item.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
        SendMessageW(self.list, LVM_SETITEMSTATE, usize::MAX, &item as *const _ as LPARAM);
        let pos = self.selected.and_then(|sel| self.view.iter().position(|&i| i == sel));
        match pos {
            Some(p) => {
                item.state = LVIS_SELECTED | LVIS_FOCUSED;
                SendMessageW(self.list, LVM_SETITEMSTATE, p, &item as *const _ as LPARAM);
                SendMessageW(self.list, LVM_ENSUREVISIBLE, p, 0);
            }
            None => self.selected = None,
        }
        self.show_detail();
    }

    unsafe fn on_selection_changed(&mut self) {
        let row = SendMessageW(self.list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as LPARAM);
        let sel = usize::try_from(row).ok().and_then(|r| self.view.get(r).copied());
        if sel != self.selected {
            self.selected = sel;
            self.show_detail();
        }
    }

    unsafe fn show_detail(&self) {
        let Some(e) = self.selected.map(|i| &self.entries[i]) else {
            for h in [self.caption, self.command, self.location, self.info] {
                set_text(h, "");
            }
            for h in [self.caption, self.command, self.save, self.toggle, self.open] {
                EnableWindow(h, 0);
            }
            set_text(self.info, "Select an item to see where it comes from.");
            return;
        };
        set_text(self.caption, &e.title);
        set_text(self.command, &e.command);
        set_text(self.location, &e.location());
        let can_caption = e.caption_value.is_some();
        let can_command = e.command_type.is_some();
        SendMessageW(self.caption, EM_SETREADONLY, !can_caption as usize, 0);
        SendMessageW(self.command, EM_SETREADONLY, !can_command as usize, 0);
        EnableWindow(self.caption, 1);
        EnableWindow(self.command, 1);
        EnableWindow(self.save, (can_caption || can_command) as i32);
        EnableWindow(self.toggle, 1);
        EnableWindow(self.open, 1);
        let (toggle, open) = match (e.kind, e.disabled) {
            (Kind::Verb, false) => ("Disable", "Open in Regedit"),
            (Kind::Verb, true) => ("Enable", "Open in Regedit"),
            (Kind::Handler, false) => ("Block", "Open in Regedit"),
            (Kind::Handler, true) => ("Unblock", "Open in Regedit"),
            (Kind::Packaged, false) => ("Block", "Show manifest"),
            (Kind::Packaged, true) => ("Unblock", "Show manifest"),
        };
        set_text(self.toggle, toggle);
        set_text(self.open, open);
        let note = match e.kind {
            Kind::Verb if can_command => {
                "Classic menu (\"Show more options\"). HKCU entries override HKLM. Disable = LegacyDisable value."
            }
            Kind::Verb => "Classic menu (\"Show more options\"). The action is a COM object, so there is no command line to edit.",
            Kind::Handler => {
                "COM shell extension: its menu text comes from the DLL at runtime. Block = HKCU Shell Extensions\\Blocked (restart Explorer)."
            }
            Kind::Packaged => {
                "Windows 11 top-level menu, declared in the app's signed package manifest (read-only). Block hides it (restart Explorer)."
            }
        };
        set_text(self.info, note);
    }

    /// Returns false if the write failed (the user has been told why).
    unsafe fn check(&self, r: Result<(), u32>) -> bool {
        let Err(code) = r else { return true };
        if code == ERROR_ACCESS_DENIED && !self.is_admin {
            let msg = wide(
                "Access denied.\n\nKeys under HKEY_LOCAL_MACHINE can only be changed by an administrator.\n\n\
                 Restart this tool as administrator?",
            );
            let answer = MessageBoxW(self.hwnd, msg.as_ptr(), w!("Context Menu Editor"), MB_YESNO | MB_ICONWARNING);
            if answer == IDYES {
                let mut path = [0u16; 1024];
                GetModuleFileNameW(null_mut(), path.as_mut_ptr(), path.len() as u32);
                let r = ShellExecuteW(self.hwnd, w!("runas"), path.as_ptr(), null(), null(), SW_SHOWNORMAL);
                if r as isize > 32 {
                    PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
                }
            }
        } else {
            let msg = wide(&format!("Registry write failed: {}.", error_text(code)));
            MessageBoxW(self.hwnd, msg.as_ptr(), w!("Context Menu Editor"), MB_OK | MB_ICONERROR);
        }
        false
    }

    unsafe fn save(&mut self) {
        let Some(e) = self.selected.map(|i| self.entries[i].clone()) else { return };
        let caption = get_text(self.caption);
        let command = get_text(self.command);
        let mut changed = false;
        if let Some(value) = &e.caption_value {
            if caption != e.title {
                if !self.check(reg::set_str(e.hive, &e.key, value, &caption, REG_SZ)) {
                    return;
                }
                changed = true;
            }
        }
        if let Some(ty) = e.command_type {
            if command != e.command {
                if !self.check(reg::set_str(e.hive, &format!("{}\\command", e.key), "", &command, ty)) {
                    return;
                }
                changed = true;
            }
        }
        if changed {
            self.rescan();
            set_text(self.info, "Saved. Explorer picks the change up on the next right-click.");
        }
    }

    unsafe fn toggle(&mut self) {
        let Some(e) = self.selected.map(|i| self.entries[i].clone()) else { return };
        let ok = match (e.kind, &e.clsid) {
            (Kind::Verb, _) if e.disabled => self.check(reg::delete_value(e.hive, &e.key, "LegacyDisable")),
            (Kind::Verb, _) => self.check(reg::set_str(e.hive, &e.key, "LegacyDisable", "", REG_SZ)),
            (_, Some(clsid)) if e.disabled => {
                let in_hklm = reg::Key::open(Hive::LocalMachine, BLOCKED_KEY).is_some_and(|k| k.has_value(clsid));
                self.check(reg::delete_value(Hive::CurrentUser, BLOCKED_KEY, clsid))
                    && (!in_hklm || self.check(reg::delete_value(Hive::LocalMachine, BLOCKED_KEY, clsid)))
            }
            (_, Some(clsid)) => self.check(reg::set_str(Hive::CurrentUser, BLOCKED_KEY, clsid, &e.caption, REG_SZ)),
            _ => false,
        };
        if ok {
            self.rescan();
            if e.kind != Kind::Verb {
                set_text(self.info, "Done. Restart Explorer (or sign out) for shell-extension changes to apply.");
            }
        }
    }

    unsafe fn open_location(&self) {
        let Some(e) = self.selected.map(|i| &self.entries[i]) else { return };
        if e.kind == Kind::Packaged {
            let args = wide(&format!("/select,\"{}\"", e.key));
            ShellExecuteW(self.hwnd, null(), w!("explorer.exe"), args.as_ptr(), null(), SW_SHOWNORMAL);
            return;
        }
        // Regedit reopens at its LastKey; /m starts a new instance so that it is honoured.
        let last = format!("Computer\\{}\\{}", e.hive.long(), e.key);
        let applet = r"Software\Microsoft\Windows\CurrentVersion\Applets\Regedit";
        let _ = reg::set_str(Hive::CurrentUser, applet, "LastKey", &last, REG_SZ);
        ShellExecuteW(self.hwnd, null(), w!("regedit.exe"), w!("/m"), null(), SW_SHOWNORMAL);
    }

    unsafe fn on_notify(&mut self, lparam: LPARAM) -> LRESULT {
        let hdr = &*(lparam as *const NMHDR);
        if hdr.idFrom != ID_LIST as usize {
            return 0;
        }
        match hdr.code {
            LVN_GETDISPINFOW => {
                let di = &mut *(lparam as *mut NMLVDISPINFOW);
                if di.item.mask & LVIF_TEXT != 0 && di.item.cchTextMax > 0 {
                    let text = usize::try_from(di.item.iItem)
                        .ok()
                        .and_then(|r| self.view.get(r))
                        .map(|&i| self.entries[i].column(di.item.iSubItem as usize))
                        .unwrap_or_default();
                    let max = di.item.cchTextMax as usize - 1;
                    let mut n = 0;
                    for (k, c) in text.encode_utf16().take(max).enumerate() {
                        *di.item.pszText.add(k) = c;
                        n = k + 1;
                    }
                    *di.item.pszText.add(n) = 0;
                }
            }
            LVN_ITEMCHANGED | LVN_ODSTATECHANGED => {
                PostMessageW(self.hwnd, WM_APP_SELCHANGED, 0, 0);
            }
            LVN_COLUMNCLICK => {
                let col = (*(lparam as *const NMLISTVIEW)).iSubItem as usize;
                self.sort = match self.sort {
                    Some((c, asc)) if c == col => Some((col, !asc)),
                    _ => Some((col, true)),
                };
                self.rebuild_view();
            }
            NM_DBLCLK => self.open_location(),
            _ => {}
        }
        0
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        (*(cs.lpCreateParams as *mut App)).hwnd = hwnd;
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let app = &mut *ptr;
    match msg {
        WM_CREATE => {
            app.create();
            PostMessageW(hwnd, WM_APP_SCAN, 0, 0);
            0
        }
        WM_SIZE => {
            app.layout();
            0
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lparam as *mut MINMAXINFO);
            mmi.ptMinTrackSize = POINT { x: app.s(900), y: app.s(480) };
            0
        }
        WM_APP_SCAN => {
            app.rescan();
            0
        }
        WM_APP_SELCHANGED => {
            app.on_selection_changed();
            0
        }
        WM_NOTIFY => app.on_notify(lparam),
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as i32;
            let code = ((wparam >> 16) & 0xFFFF) as u32;
            match (id, code) {
                (ID_FILTER, EN_CHANGE) => app.rebuild_view(),
                (ID_FILE_TYPES, BN_CLICKED) | (ID_REFRESH, BN_CLICKED) => app.rescan(),
                (ID_SAVE, BN_CLICKED) => app.save(),
                (ID_TOGGLE, BN_CLICKED) => app.toggle(),
                (ID_OPEN, BN_CLICKED) => app.open_location(),
                // Enter pressed inside the caption / command edit.
                (IDOK, _) if GetFocus() == app.caption || GetFocus() == app.command => app.save(),
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DeleteObject(app.font);
            drop(Box::from_raw(ptr));
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    unsafe {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&icc);

        let hinst = GetModuleHandleW(null());
        let class = w!("WindowsMenuEditor");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_BTNFACE + 1) as isize as HBRUSH,
            lpszClassName: class,
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);

        let is_admin = IsUserAnAdmin() != 0;
        let app = Box::new(App {
            hwnd: null_mut(),
            font: null_mut(),
            scale: GetDpiForSystem() as f32 / 96.0,
            list: null_mut(),
            filter_label: null_mut(),
            filter: null_mut(),
            file_types: null_mut(),
            refresh: null_mut(),
            count: null_mut(),
            labels: [null_mut(); 4],
            caption: null_mut(),
            command: null_mut(),
            location: null_mut(),
            info: null_mut(),
            save: null_mut(),
            toggle: null_mut(),
            open: null_mut(),
            entries: Vec::new(),
            view: Vec::new(),
            selected: None,
            sort: None,
            is_admin,
        });
        let scale = app.scale;
        let title = wide(if is_admin {
            "Windows Context Menu Editor (Administrator)"
        } else {
            "Windows Context Menu Editor"
        });
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class,
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            (1300.0 * scale) as i32,
            (800.0 * scale) as i32,
            null_mut(),
            null_mut(),
            hinst,
            Box::into_raw(app) as *const _,
        );
        ShowWindow(hwnd, SW_SHOWNORMAL);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
