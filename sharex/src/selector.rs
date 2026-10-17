//! Full-screen overlay over a frozen screenshot where the user picks the
//! capture area: hover + click selects a window (or a monitor), drag selects
//! a free rectangle. Esc / right click cancels.

use std::cell::Cell;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::capture::Screenshot;
use crate::config::Config;
use crate::gfx::{self, Dib};
use crate::util::*;
use crate::{editor, output};

const CLASS: &str = "RustShot.Selector";
const WM_APP_FINISH: u32 = WM_APP + 10;
const ACCENT: u32 = 0x0A84FF;
const DRAG_THRESHOLD: i32 = 4;
const MAG_SRC: i32 = 15; // magnified source pixels (odd)
const MAG_ZOOM: i32 = 8;

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub fn is_active() -> bool {
    ACTIVE.with(|a| a.get())
}

struct State {
    cfg: Config,
    shot: Screenshot,
    dim: Dib,
    back: Dib,
    /// Candidate rectangles, screenshot-local coords, Z order.
    targets: Vec<RECT>,
    mouse: (i32, i32),
    down: Option<(i32, i32)>,
    dragging: bool,
    /// Area drawn with the decorations at the last paint (for invalidation).
    last_decor: Option<RECT>,
    font: HFONT,
    dpi: f32,
    result: Option<RECT>,
}

impl State {
    fn hover_target(&self) -> Option<RECT> {
        let (x, y) = self.mouse;
        self.targets.iter().find(|r| rect_contains(r, x, y)).copied()
    }

    /// The rectangle currently shown as selected.
    fn selection(&self) -> Option<RECT> {
        if self.dragging {
            let (sx, sy) = self.down?;
            let (x, y) = self.mouse;
            let r = rect(sx.min(x), sy.min(y), sx.max(x) + 1, sy.max(y) + 1);
            rect_intersect(&r, &rect(0, 0, self.shot.img.w, self.shot.img.h))
        } else {
            self.hover_target()
        }
    }

    fn label_rect(&self, sel: &RECT) -> RECT {
        let (w, h) = ((150.0 * self.dpi) as i32, (26.0 * self.dpi) as i32);
        let top = if sel.top - h - 6 >= 0 { sel.top - h - 6 } else { sel.top + 6 };
        let left = sel.left.clamp(0, (self.shot.img.w - w).max(0));
        rect(left, top, left + w, top + h)
    }

    fn magnifier_rect(&self) -> RECT {
        let size = MAG_SRC * MAG_ZOOM;
        let (x, y) = self.mouse;
        let off = (24.0 * self.dpi) as i32;
        let mut l = x + off;
        let mut t = y + off;
        // Flip to the other side near the right/bottom edge of the monitor.
        let work = work_area_at(POINT { x: x + self.shot.x, y: y + self.shot.y });
        if l + size + self.shot.x > work.right {
            l = x - off - size;
        }
        if t + size + self.shot.y > work.bottom {
            t = y - off - size;
        }
        rect(l, t, l + size, t + size)
    }

    fn decor_bounds(&self) -> Option<RECT> {
        let mut b: Option<RECT> = None;
        let mut add = |r: RECT| b = Some(b.map_or(r, |b| rect_union(&b, &r)));
        if let Some(sel) = self.selection() {
            add(rect_inflate(&sel, 3));
            add(self.label_rect(&sel));
        }
        if self.cfg.capture.show_magnifier {
            add(rect_inflate(&self.magnifier_rect(), 2));
        }
        b
    }
}

pub fn open(shot: Screenshot, targets_abs: Vec<RECT>, cfg: Config) {
    unsafe {
        let hinst = hinstance();
        let class = wide(CLASS);
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinst;
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(null_mut(), IDC_CROSS);
        RegisterClassW(&wc); // fails harmlessly if already registered

        let targets = targets_abs
            .iter()
            .map(|r| rect(r.left - shot.x, r.top - shot.y, r.right - shot.x, r.bottom - shot.y))
            .collect();
        let dim = shot.dimmed(cfg.capture.dim_opacity);
        let back = Dib::new(shot.img.w, shot.img.h);
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let (x, y, w, h) = (shot.x, shot.y, shot.img.w, shot.img.h);
        let state = Box::new(State {
            mouse: (pt.x - shot.x, pt.y - shot.y),
            cfg,
            shot,
            dim,
            back,
            targets,
            down: None,
            dragging: false,
            last_decor: None,
            font: null_mut(),
            dpi: 1.0,
            result: None,
        });

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide("RustShot selection").as_ptr(),
            WS_POPUP,
            x,
            y,
            w,
            h,
            null_mut(),
            null_mut(),
            hinst,
            Box::into_raw(state) as *const _,
        );
        if hwnd.is_null() {
            return;
        }
        ACTIVE.with(|a| a.set(true));
        ShowWindow(hwnd, SW_SHOW);
        force_foreground(hwnd);
    }
}

/// SetForegroundWindow with the AttachThreadInput fallback (needed when the
/// capture was triggered by the low-level keyboard hook).
pub fn force_foreground(hwnd: HWND) {
    unsafe {
        if SetForegroundWindow(hwnd) != 0 {
            return;
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, null_mut());
        let me = GetCurrentThreadId();
        AttachThreadInput(me, fg_thread, 1);
        SetForegroundWindow(hwnd);
        BringWindowToTop(hwnd);
        AttachThreadInput(me, fg_thread, 0);
    }
}

unsafe fn state<'a>(hwnd: HWND) -> Option<&'a mut State> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State).as_mut()
}

fn update(hwnd: HWND, st: &mut State) {
    let new = st.decor_bounds();
    unsafe {
        for r in [st.last_decor, new].into_iter().flatten() {
            InvalidateRect(hwnd, &r, 0);
        }
    }
}

fn finish(hwnd: HWND, st: &mut State, result: Option<RECT>) {
    st.result = result;
    unsafe { PostMessageW(hwnd, WM_APP_FINISH, 0, 0) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        if let Some(st) = state(hwnd) {
            st.dpi = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            let face = wide("Segoe UI");
            st.font = CreateFontW(
                -(15.0 * st.dpi) as i32, 0, 0, 0, FW_SEMIBOLD as i32, 0, 0, 0, DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32,
                (DEFAULT_PITCH | FF_DONTCARE) as u32, face.as_ptr(),
            );
        }
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let Some(st) = state(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd, st);
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = lparam_xy(lp);
            st.mouse = (x, y);
            if let Some((sx, sy)) = st.down {
                if !st.dragging && ((x - sx).abs() > DRAG_THRESHOLD || (y - sy).abs() > DRAG_THRESHOLD) {
                    st.dragging = true;
                }
            }
            update(hwnd, st);
            0
        }
        WM_LBUTTONDOWN => {
            st.down = Some(lparam_xy(lp));
            st.mouse = lparam_xy(lp);
            SetCapture(hwnd);
            0
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            if st.down.is_none() {
                return 0;
            }
            st.mouse = lparam_xy(lp);
            let sel = st.selection();
            st.down = None;
            st.dragging = false;
            match sel {
                Some(r) if rect_w(&r) >= 2 && rect_h(&r) >= 2 => finish(hwnd, st, Some(r)),
                _ => update(hwnd, st),
            }
            0
        }
        WM_RBUTTONDOWN => {
            if st.dragging || st.down.is_some() {
                ReleaseCapture();
                st.down = None;
                st.dragging = false;
                update(hwnd, st);
            } else {
                finish(hwnd, st, None);
            }
            0
        }
        WM_KEYDOWN => {
            let vk = wp as u16;
            if vk == VK_ESCAPE {
                finish(hwnd, st, None);
            } else if vk == VK_RETURN {
                let sel = st.selection();
                finish(hwnd, st, sel);
            } else if matches!(vk, VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN) {
                // Pixel-precise cursor nudging.
                let step = if shift_down() { 10 } else { 1 };
                let (dx, dy) = match vk {
                    VK_LEFT => (-step, 0),
                    VK_RIGHT => (step, 0),
                    VK_UP => (0, -step),
                    _ => (0, step),
                };
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                SetCursorPos(pt.x + dx, pt.y + dy);
            }
            0
        }
        WM_APP_FINISH => {
            let result = st.result;
            let crop = result.map(|r| {
                let (w, h, px) = st.shot.crop(&r);
                (output::Image { w, h, bgra: px }, rect(r.left + st.shot.x, r.top + st.shot.y, r.right + st.shot.x, r.bottom + st.shot.y))
            });
            let cfg = st.cfg.clone();
            // `st` must not be used after this point: destroying frees it.
            DestroyWindow(hwnd);
            if let Some((img, screen_rect)) = crop {
                editor::open(img, screen_rect, cfg);
            }
            0
        }
        WM_NCDESTROY => {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !p.is_null() {
                let st = Box::from_raw(p);
                if !st.font.is_null() {
                    DeleteObject(st.font);
                }
            }
            ACTIVE.with(|a| a.set(false));
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn shift_down() -> bool {
    unsafe { GetKeyState(VK_SHIFT as i32) < 0 }
}

unsafe fn paint(hwnd: HWND, st: &mut State) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let r = ps.rcPaint;
    let (rw, rh) = (rect_w(&r), rect_h(&r));
    let back = st.back.dc;

    BitBlt(back, r.left, r.top, rw, rh, st.dim.dc, r.left, r.top, SRCCOPY);

    let sel = st.selection();
    if let Some(sel) = sel {
        if let Some(i) = rect_intersect(&sel, &r) {
            BitBlt(back, i.left, i.top, rect_w(&i), rect_h(&i), st.shot.img.dc, i.left, i.top, SRCCOPY);
        }
        // Border just outside the selection.
        let pen = CreatePen(PS_SOLID, 2, gfx::colorref(ACCENT));
        let old_pen = SelectObject(back, pen);
        let old_brush = SelectObject(back, GetStockObject(NULL_BRUSH));
        Rectangle(back, sel.left - 1, sel.top - 1, sel.right + 1, sel.bottom + 1);
        SelectObject(back, old_brush);
        SelectObject(back, old_pen);
        DeleteObject(pen);

        // Size label.
        let lr = st.label_rect(&sel);
        gfx::fill_rect(back, &lr, 0x1C1C1E);
        let text = wide(&format!("{} × {}", rect_w(&sel), rect_h(&sel)));
        let old_font = SelectObject(back, st.font);
        SetBkMode(back, TRANSPARENT as i32);
        SetTextColor(back, 0xFFFFFF);
        let mut tr = lr;
        DrawTextW(back, text.as_ptr(), -1, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(back, old_font);
    }

    if st.cfg.capture.show_magnifier {
        let m = st.magnifier_rect();
        let (mx, my) = st.mouse;
        let half = MAG_SRC / 2;
        SetStretchBltMode(back, COLORONCOLOR);
        StretchBlt(
            back, m.left, m.top, rect_w(&m), rect_h(&m),
            st.shot.img.dc, mx - half, my - half, MAG_SRC, MAG_SRC, SRCCOPY,
        );
        // Centre-pixel marker + frame.
        let c = rect(m.left + half * MAG_ZOOM, m.top + half * MAG_ZOOM, m.left + (half + 1) * MAG_ZOOM, m.top + (half + 1) * MAG_ZOOM);
        let frame = |rc: &RECT, col: u32| {
            let br = CreateSolidBrush(gfx::colorref(col));
            FrameRect(back, rc, br);
            DeleteObject(br);
        };
        frame(&c, ACCENT);
        frame(&m, 0xFFFFFF);
        frame(&rect_inflate(&m, 1), 0x000000);
    }

    BitBlt(dc, r.left, r.top, rw, rh, back, r.left, r.top, SRCCOPY);
    EndPaint(hwnd, &ps);
    st.last_decor = st.decor_bounds();
}
