//! Annotation window: shows the captured area and lets the user draw arrows,
//! rectangles, text and free-hand strokes, then copy / save / discard.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{Config, Drawing, EditorKeys};
use crate::gfx::{self, argb, Dib, Font, Graphics, P};
use crate::keys::KeyCombo;
use crate::output::{self, Image};
use crate::util::*;
use crate::{app, selector};

const CLASS: &str = "RustShot.Editor";
const BG: u32 = 0x202020;
const TOOLBAR_BG: u32 = 0x2B2B2D;
const ICON: u32 = 0xE8E8E8;
const ACCENT: u32 = 0x0A84FF;
const CARET_TIMER: usize = 1;
const WM_MOUSELEAVE: u32 = 0x02A3;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Move,
    Arrow,
    Rect,
    Text,
    Pen,
}

impl Tool {
    fn from_name(s: &str) -> Tool {
        match s.to_ascii_lowercase().as_str() {
            "rect" | "rectangle" | "box" => Tool::Rect,
            "text" => Tool::Text,
            "pen" | "freehand" | "pencil" => Tool::Pen,
            "move" | "hand" | "select" => Tool::Move,
            _ => Tool::Arrow,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Shape {
    Arrow { a: P, b: P, color: u32, width: f32 },
    Rect { a: P, b: P, color: u32, width: f32 },
    Pen { pts: Vec<P>, color: u32, width: f32 },
    Text { pos: P, text: String, color: u32, size: f32 },
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Item {
    Tool(Tool),
    Color(usize),
    Size(usize),
    Undo,
    Redo,
    Copy,
    Save,
    Close,
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Copy,
    Save,
    Close,
}

/// A shape being dragged with the Move tool.
struct MoveDrag {
    index: usize,
    last: P,
    before: Vec<Shape>,
    moved: bool,
}

struct TextEdit {
    pos: P,
    text: String,
    color: u32,
    size: f32,
    /// Byte index of the caret in `text`.
    caret: usize,
    /// Shapes before the edit started (restored on undo, pushed to history on change).
    before: Vec<Shape>,
    /// Position in `shapes` of the text being re-edited (None = new text).
    index: Option<usize>,
    /// The shape as it was before re-editing, to detect "no change".
    original: Option<Shape>,
}

impl TextEdit {
    /// (line number, byte index where the caret's line starts)
    fn line_start(&self) -> (usize, usize) {
        let before = &self.text[..self.caret];
        (before.matches('\n').count(), before.rfind('\n').map_or(0, |i| i + 1))
    }

    fn line_end(&self) -> usize {
        self.text[self.caret..].find('\n').map_or(self.text.len(), |i| self.caret + i)
    }

    fn prev(&self) -> usize {
        self.text[..self.caret].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.caret..].chars().next().map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    fn insert(&mut self, s: &str) {
        self.text.insert_str(self.caret, s);
        self.caret += s.len();
    }
}

struct State {
    cfg: Config,
    keys: EditorKeys,
    palette: Vec<u32>,
    img: Dib,
    /// Cached down-scaled copy of `img` for the current view scale.
    scaled: Option<(f32, Dib)>,
    back: Option<Dib>,
    // view
    ox: f32,
    oy: f32,
    scale: f32,
    dpi: f32,
    tb_h: i32,
    items: Vec<(RECT, Item)>,
    hover: Option<usize>,
    tracking: bool,
    // document
    shapes: Vec<Shape>,
    /// Snapshots of `shapes` for undo / redo.
    undo: Vec<Vec<Shape>>,
    redo: Vec<Vec<Shape>>,
    tool: Tool,
    color: usize,
    size: usize,
    drag: Option<Shape>,
    /// Shape selected with the Move tool.
    selected: Option<usize>,
    moving: Option<MoveDrag>,
    text: Option<TextEdit>,
    caret_on: bool,
    /// Set once a final action ran; the window is hidden and about to close.
    closing: bool,
    high_surrogate: Option<u16>,
    ui_font: Option<Font>,
}

// ---------------------------------------------------------------------------
// Opening
// ---------------------------------------------------------------------------

pub fn open(img: Image, screen_rect: RECT, cfg: Config) {
    unsafe {
        let hinst = hinstance();
        let class = wide(CLASS);
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinst;
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hIcon = crate::tray::app_icon(GetSystemMetrics(SM_CXICON));
        wc.hIconSm = crate::tray::app_icon(GetSystemMetrics(SM_CXSMICON));
        RegisterClassExW(&wc);

        let mut errors = Vec::new();
        let keys = cfg.editor_keys(&mut errors);
        let palette = cfg.palette();
        let d = &cfg.drawing;
        let state = Box::new(State {
            keys,
            color: d.default_color.min(palette.len() - 1),
            size: d.default_size.min(2),
            tool: Tool::from_name(&d.default_tool),
            palette,
            img: Dib::from_bgra(img.w, img.h, &img.bgra),
            scaled: None,
            back: None,
            ox: 0.0,
            oy: 0.0,
            scale: 1.0,
            dpi: 1.0,
            tb_h: 44,
            items: Vec::new(),
            hover: None,
            tracking: false,
            shapes: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            selected: None,
            moving: None,
            text: None,
            caret_on: true,
            closing: false,
            high_surrogate: None,
            ui_font: None,
            cfg,
        });

        let title = wide(&title_for(&state, img.w, img.h));
        let style = WS_OVERLAPPEDWINDOW;
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            style,
            screen_rect.left,
            screen_rect.top,
            100,
            100,
            null_mut(),
            null_mut(),
            hinst,
            Box::into_raw(state) as *const _,
        );
        if hwnd.is_null() {
            return;
        }
        place_window(hwnd, &screen_rect, img.w, img.h);
        ShowWindow(hwnd, SW_SHOW);
        selector::force_foreground(hwnd);
        if !errors.is_empty() {
            app::notify("Keybinding errors in config.yml", &errors.join("\n"), true);
        }
    }
}

fn title_for(st: &State, w: i32, h: i32) -> String {
    format!(
        "{APP_NAME} — {w} × {h}   ·   {}: copy   ·   {}: save   ·   {}: discard",
        st.keys.save_to_clipboard.label(),
        st.keys.save_to_default_location.label(),
        st.keys.close_without_saving.label()
    )
}

/// Size the window to fit the image (scaled down if needed) and put the
/// image exactly where it was captured when that fits on the monitor.
unsafe fn place_window(hwnd: HWND, sel: &RECT, iw: i32, ih: i32) {
    let Some(st) = state(hwnd) else { return };
    let dpi = GetDpiForWindow(hwnd).max(96);
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let mut frame = rect(0, 0, 0, 0);
    AdjustWindowRectExForDpi(&mut frame, style, 0, 0, dpi);
    let (fw, fh) = (rect_w(&frame), rect_h(&frame));

    let center = POINT { x: (sel.left + sel.right) / 2, y: (sel.top + sel.bottom) / 2 };
    let work = work_area_at(center);
    let tb = st.tb_h;
    let min_w = toolbar_min_width(st);
    let avail_w = (rect_w(&work) - fw) as f32;
    let avail_h = (rect_h(&work) - fh - tb) as f32;
    let scale = 1f32.min(avail_w / iw as f32).min(avail_h / ih as f32).max(0.05);
    let cw = ((iw as f32 * scale).ceil() as i32).max(min_w);
    let ch = (ih as f32 * scale).ceil() as i32 + tb;
    let (ww, wh) = (cw + fw, ch + fh);

    // Where the window must be for the image to stay in place.
    let img_off_x = (cw - (iw as f32 * scale) as i32) / 2;
    let mut x = sel.left - (-frame.left) - img_off_x;
    let mut y = sel.top - (-frame.top) - tb;
    if scale < 1.0 || x < work.left || y < work.top || x + ww > work.right || y + wh > work.bottom {
        x = x.clamp(work.left, (work.right - ww).max(work.left));
        y = y.clamp(work.top, (work.bottom - wh).max(work.top));
        if scale < 1.0 {
            x = work.left + (rect_w(&work) - ww) / 2;
            y = work.top + (rect_h(&work) - wh) / 2;
        }
    }
    SetWindowPos(hwnd, null_mut(), x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
}

// ---------------------------------------------------------------------------
// Layout & coordinates
// ---------------------------------------------------------------------------

fn px(st: &State, v: f32) -> i32 {
    (v * st.dpi).round() as i32
}

fn toolbar_min_width(st: &State) -> i32 {
    let (btn, gap, group) = (px(st, 32.0), px(st, 2.0), px(st, 12.0));
    let n_left = 5 + st.palette.len().min(9) + 3 + 2;
    px(st, 8.0) * 2 + (n_left as i32 + 3) * (btn + gap) + 4 * group
}

fn layout(hwnd: HWND, st: &mut State) {
    let mut cr = rect(0, 0, 0, 0);
    unsafe { GetClientRect(hwnd, &mut cr) };
    let (cw, ch) = (rect_w(&cr).max(1), rect_h(&cr).max(1));

    let pad = px(st, 6.0);
    let btn = px(st, 32.0);
    let gap = px(st, 2.0);
    let group = px(st, 12.0);
    st.tb_h = btn + 2 * pad;

    let mut items = Vec::new();
    let mut x = px(st, 8.0);
    let y = pad;
    let push = |items: &mut Vec<(RECT, Item)>, x: &mut i32, it: Item| {
        items.push((rect(*x, y, *x + btn, y + btn), it));
        *x += btn + gap;
    };
    for t in [Tool::Move, Tool::Arrow, Tool::Rect, Tool::Text, Tool::Pen] {
        push(&mut items, &mut x, Item::Tool(t));
    }
    x += group;
    for i in 0..st.palette.len().min(9) {
        push(&mut items, &mut x, Item::Color(i));
    }
    x += group;
    for i in 0..3 {
        push(&mut items, &mut x, Item::Size(i));
    }
    x += group;
    push(&mut items, &mut x, Item::Undo);
    push(&mut items, &mut x, Item::Redo);
    x += group;
    let right = cw - px(st, 8.0) - 3 * (btn + gap);
    let mut x = x.max(right);
    for it in [Item::Copy, Item::Save, Item::Close] {
        push(&mut items, &mut x, it);
    }
    st.items = items;

    // Fit image below the toolbar, never upscale.
    let (iw, ih) = (st.img.w as f32, st.img.h as f32);
    let avail_h = (ch - st.tb_h).max(1) as f32;
    st.scale = 1f32.min(cw as f32 / iw).min(avail_h / ih).max(0.01);
    st.ox = ((cw as f32 - iw * st.scale) / 2.0).floor();
    st.oy = st.tb_h as f32 + ((avail_h - ih * st.scale) / 2.0).floor();
}

fn to_image(st: &State, x: i32, y: i32) -> P {
    P::new((x as f32 - st.ox) / st.scale, (y as f32 - st.oy) / st.scale)
}

fn item_at(st: &State, x: i32, y: i32) -> Option<usize> {
    st.items.iter().position(|(r, _)| rect_contains(r, x, y))
}

fn line_width(st: &State) -> f32 {
    st.cfg.drawing.line_widths[st.size].max(0.5)
}

fn font_size(st: &State) -> f32 {
    st.cfg.drawing.font_sizes[st.size].max(4.0)
}

fn current_color(st: &State) -> u32 {
    st.palette[st.color.min(st.palette.len() - 1)]
}

// ---------------------------------------------------------------------------
// Shape rendering (image coordinates)
// ---------------------------------------------------------------------------

fn arrow_geometry(a: P, b: P, w: f32) -> Option<(P, [P; 3])> {
    let len = a.dist(b);
    if len < 1.0 {
        return None;
    }
    let (dx, dy) = ((b.x - a.x) / len, (b.y - a.y) / len);
    let head = (w * 3.0 + 9.0).min(len);
    let hw = head * 0.5;
    let base = P::new(b.x - dx * head, b.y - dy * head);
    let shaft_end = P::new(b.x - dx * head * 0.7, b.y - dy * head * 0.7);
    let tri = [b, P::new(base.x - dy * hw, base.y + dx * hw), P::new(base.x + dy * hw, base.y - dx * hw)];
    Some((shaft_end, tri))
}

fn draw_arrow(g: &Graphics, a: P, b: P, color: u32, w: f32) {
    if let Some((shaft_end, tri)) = arrow_geometry(a, b, w) {
        g.line(a, shaft_end, color, w);
        g.fill_polygon(&tri, color);
    }
}

fn norm_rect(a: P, b: P) -> (f32, f32, f32, f32) {
    (a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
}

fn text_halo(color: u32) -> u32 {
    if gfx::luminance(color) > 0.6 {
        argb(0x101010, 0xD0)
    } else {
        argb(0xFFFFFF, 0xE6)
    }
}

/// Background box colour that contrasts with the text colour:
/// white behind dark/saturated text, near-black behind light text.
fn text_background(color: u32) -> u32 {
    if gfx::luminance(color) > 0.5 {
        0x1E1E1E
    } else {
        0xFFFFFF
    }
}

struct TextLayout {
    line_h: f32,
    widths: Vec<f32>,
}

fn text_font(family: &str, size: f32) -> Option<Font> {
    Font::new(family, size, true)
}

fn text_layout(g: &Graphics, text: &str, font: &Font) -> TextLayout {
    TextLayout { line_h: g.measure("Ag", font).1, widths: text.split('\n').map(|l| g.measure(l, font).0).collect() }
}

/// (x, y, w, h) of a text including its background padding.
fn text_box(l: &TextLayout, pos: P, size: f32) -> (f32, f32, f32, f32) {
    let (px, py) = (size * 0.3, size * 0.12);
    let maxw = l.widths.iter().cloned().fold(size * 0.4, f32::max);
    (pos.x - px, pos.y - py, maxw + 2.0 * px, l.line_h * l.widths.len() as f32 + 2.0 * py)
}

fn box_contains((x, y, w, h): (f32, f32, f32, f32), p: P) -> bool {
    p.x >= x && p.x <= x + w && p.y >= y && p.y <= y + h
}

/// Byte index in `text` closest to point `p`.
fn caret_at(g: &Graphics, text: &str, pos: P, font: &Font, l: &TextLayout, p: P) -> usize {
    let line = (((p.y - pos.y) / l.line_h).floor().max(0.0) as usize).min(l.widths.len() - 1);
    let start: usize = text.split('\n').take(line).map(|s| s.len() + 1).sum();
    let line_str = text.split('\n').nth(line).unwrap_or("");
    let mut best = (start, (pos.x - p.x).abs());
    for (i, c) in line_str.char_indices() {
        let end = i + c.len_utf8();
        let d = (pos.x + g.measure(&line_str[..end], font).0 - p.x).abs();
        if d < best.1 {
            best = (start + end, d);
        }
    }
    best.0
}

fn draw_text(g: &Graphics, pos: P, text: &str, color: u32, size: f32, d: &Drawing) {
    let Some(font) = text_font(&d.font_family, size) else { return };
    let l = text_layout(g, text, &font);
    let outline = if d.text_background {
        let (x, y, w, h) = text_box(&l, pos, size);
        let r = size * 0.2;
        if d.shadow {
            let o = (size * 0.08).max(1.5);
            g.fill_round_rect(x + o, y + o, w, h, r, argb(0, 0x50));
        }
        g.fill_round_rect(x, y, w, h, r, argb(text_background(color), 0xF5));
        None
    } else {
        Some((text_halo(color), (size * 0.16).max(2.0)))
    };
    for (i, line) in text.split('\n').enumerate() {
        g.outlined_text(line, pos.x, pos.y + l.line_h * i as f32, &font, argb(color, 255), outline);
    }
}

fn draw_shape(g: &Graphics, s: &Shape, d: &Drawing) {
    let shadow = d.shadow;
    let sh = |w: f32| (w * 0.4).max(1.5);
    let shadow_color = argb(0, 0x50);
    match s {
        Shape::Arrow { a, b, color, width } => {
            if shadow {
                let o = sh(*width);
                draw_arrow(g, P::new(a.x + o, a.y + o), P::new(b.x + o, b.y + o), shadow_color, *width);
            }
            draw_arrow(g, *a, *b, argb(*color, 255), *width);
        }
        Shape::Rect { a, b, color, width } => {
            let (x, y, w, h) = norm_rect(*a, *b);
            if shadow {
                let o = sh(*width);
                g.rect(x + o, y + o, w, h, shadow_color, *width);
            }
            g.rect(x, y, w, h, argb(*color, 255), *width);
        }
        Shape::Pen { pts, color, width } => {
            if shadow {
                let o = sh(*width);
                let shifted: Vec<P> = pts.iter().map(|p| P::new(p.x + o, p.y + o)).collect();
                g.polyline(&shifted, shadow_color, *width);
            }
            g.polyline(pts, argb(*color, 255), *width);
        }
        Shape::Text { pos, text, color, size } => draw_text(g, *pos, text, *color, *size, d),
    }
}

/// Final image: original pixels + all shapes at 1:1.
fn render_final(st: &mut State) -> Image {
    commit_text(st);
    let d = Dib::new(st.img.w, st.img.h);
    unsafe { BitBlt(d.dc, 0, 0, d.w, d.h, st.img.dc, 0, 0, SRCCOPY) };
    {
        let g = Graphics::from_hdc(d.dc);
        for s in &st.shapes {
            draw_shape(&g, s, &st.cfg.drawing);
        }
    }
    Image { w: d.w, h: d.h, bgra: d.to_bgra_opaque() }
}

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

unsafe fn paint(hwnd: HWND, st: &mut State) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut cr = rect(0, 0, 0, 0);
    GetClientRect(hwnd, &mut cr);
    let (cw, ch) = (rect_w(&cr).max(1), rect_h(&cr).max(1));
    if st.back.as_ref().is_none_or(|b| b.w != cw || b.h != ch) {
        st.back = Some(Dib::new(cw, ch));
    }
    if st.ui_font.is_none() {
        st.ui_font = Font::new("Segoe UI", 12.5 * st.dpi, false);
    }
    let back = st.back.take().unwrap();
    gfx::fill_rect(back.dc, &cr, BG);

    // Image (cached at the current scale).
    let iw = ((st.img.w as f32) * st.scale).round().max(1.0) as i32;
    let ih = ((st.img.h as f32) * st.scale).round().max(1.0) as i32;
    if st.scale >= 0.999 {
        BitBlt(back.dc, st.ox as i32, st.oy as i32, st.img.w, st.img.h, st.img.dc, 0, 0, SRCCOPY);
    } else {
        if st.scaled.as_ref().is_none_or(|(s, _)| *s != st.scale) {
            let d = Dib::new(iw, ih);
            SetStretchBltMode(d.dc, HALFTONE);
            SetBrushOrgEx(d.dc, 0, 0, null_mut());
            StretchBlt(d.dc, 0, 0, iw, ih, st.img.dc, 0, 0, st.img.w, st.img.h, SRCCOPY);
            st.scaled = Some((st.scale, d));
        }
        let (_, d) = st.scaled.as_ref().unwrap();
        BitBlt(back.dc, st.ox as i32, st.oy as i32, d.w, d.h, d.dc, 0, 0, SRCCOPY);
    }

    let g = Graphics::from_hdc(back.dc);
    // Shapes, clipped to the image.
    g.clip(st.ox, st.oy, iw as f32, ih as f32);
    g.translate_scale(st.ox, st.oy, st.scale);
    for s in st.shapes.iter().chain(st.drag.iter()) {
        draw_shape(&g, s, &st.cfg.drawing);
    }
    if let Some(s) = st.selected.and_then(|i| st.shapes.get(i)) {
        let (x, y, w, h) = shape_bounds(&g, s, &st.cfg.drawing);
        let pad = 4.0 / st.scale;
        let lw = (1.5 / st.scale).max(1.0);
        g.rect(x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad, argb(0xFFFFFF, 0xC0), lw * 2.0);
        g.dashed_rect(x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad, argb(ACCENT, 0xFF), lw);
    }
    if let Some(t) = &st.text {
        draw_text(&g, t.pos, &t.text, t.color, t.size, &st.cfg.drawing);
        if let Some(font) = text_font(&st.cfg.drawing.font_family, t.size) {
            let l = text_layout(&g, &t.text, &font);
            let (x, y, w, h) = text_box(&l, t.pos, t.size);
            let pad = 3.0 / st.scale;
            g.dashed_rect(x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad, argb(ACCENT, 0xE0), (1.5 / st.scale).max(1.0));
            if st.caret_on {
                let (line, start) = t.line_start();
                let cx = t.pos.x + g.measure(&t.text[start..t.caret], &font).0;
                let cy = t.pos.y + l.line_h * line as f32;
                let caret_w = (2.0 / st.scale).max(1.0);
                g.line(P::new(cx, cy + l.line_h * 0.08), P::new(cx, cy + l.line_h * 0.92), argb(t.color, 255), caret_w);
            }
        }
    }
    g.reset_transform();
    g.reset_clip();

    draw_toolbar(&g, st, cw);
    drop(g);

    BitBlt(dc, 0, 0, cw, ch, back.dc, 0, 0, SRCCOPY);
    EndPaint(hwnd, &ps);
    st.back = Some(back);
}

fn tooltip(st: &State, it: Item) -> String {
    let k = &st.keys;
    let with = |name: &str, b: &crate::keys::Binding| {
        if b.0.is_empty() {
            name.to_string()
        } else {
            format!("{name}  ({})", b.label())
        }
    };
    match it {
        Item::Tool(Tool::Arrow) => with("Arrow", &k.tool_arrow),
        Item::Tool(Tool::Rect) => with("Rectangle", &k.tool_rectangle),
        Item::Tool(Tool::Text) => with("Text (click a text to edit it)", &k.tool_text),
        Item::Tool(Tool::Pen) => with("Free-hand pen", &k.tool_pen),
        Item::Tool(Tool::Move) => {
            let del = k.delete_selected.label();
            with(&format!("Move: drag a shape to move it, {del} deletes it"), &k.tool_move)
        }
        Item::Color(i) => format!("Color  ({})", i + 1),
        Item::Size(i) => ["Small", "Medium", "Large"][i].to_string(),
        Item::Undo => with("Undo", &k.undo),
        Item::Redo => with("Redo", &k.redo),
        Item::Copy => with("Copy to clipboard and close", &k.save_to_clipboard),
        Item::Save => with(&format!("Save to {}", st.cfg.default_location), &k.save_to_default_location),
        Item::Close => with("Close without saving", &k.close_without_saving),
    }
}

fn draw_toolbar(g: &Graphics, st: &State, cw: i32) {
    let d = st.dpi;
    g.fill_rect(0.0, 0.0, cw as f32, st.tb_h as f32, argb(TOOLBAR_BG, 255));
    g.fill_rect(0.0, st.tb_h as f32 - 1.0, cw as f32, 1.0, argb(0x3F3F42, 255));
    let icon = argb(ICON, 255);
    let lw = 2.0 * d;

    for (idx, (r, it)) in st.items.iter().enumerate() {
        let (x, y, s) = (r.left as f32, r.top as f32, rect_w(r) as f32);
        let at = |fx: f32, fy: f32| P::new(x + fx * s, y + fy * s);
        let hovered = st.hover == Some(idx);
        let active = match it {
            Item::Tool(t) => *t == st.tool,
            _ => false,
        };
        let enabled = match it {
            Item::Undo => !st.undo.is_empty() || st.text.is_some(),
            Item::Redo => !st.redo.is_empty(),
            _ => true,
        };
        let icon = if enabled { icon } else { argb(ICON, 0x60) };
        if active {
            g.fill_round_rect(x, y, s, s, 6.0 * d, argb(ACCENT, 255));
        } else if hovered && enabled {
            let c = if *it == Item::Close { 0xC42B1C } else { 0x3F3F42 };
            g.fill_round_rect(x, y, s, s, 6.0 * d, argb(c, 255));
        }
        match it {
            Item::Tool(Tool::Arrow) => draw_arrow(g, at(0.27, 0.73), at(0.75, 0.25), icon, lw),
            Item::Tool(Tool::Rect) => g.rect(x + 0.25 * s, y + 0.3 * s, 0.5 * s, 0.4 * s, icon, lw),
            Item::Tool(Tool::Text) => {
                g.line(at(0.3, 0.28), at(0.7, 0.28), icon, lw * 1.2);
                g.line(at(0.5, 0.28), at(0.5, 0.74), icon, lw * 1.2);
            }
            Item::Tool(Tool::Move) => {
                // Four-way move arrows.
                g.line(at(0.22, 0.5), at(0.78, 0.5), icon, lw);
                g.line(at(0.5, 0.22), at(0.5, 0.78), icon, lw);
                g.polyline(&[at(0.31, 0.41), at(0.22, 0.5), at(0.31, 0.59)], icon, lw);
                g.polyline(&[at(0.69, 0.41), at(0.78, 0.5), at(0.69, 0.59)], icon, lw);
                g.polyline(&[at(0.41, 0.31), at(0.5, 0.22), at(0.59, 0.31)], icon, lw);
                g.polyline(&[at(0.41, 0.69), at(0.5, 0.78), at(0.59, 0.69)], icon, lw);
            }
            Item::Tool(Tool::Pen) => {
                let pts = [at(0.24, 0.62), at(0.36, 0.38), at(0.5, 0.62), at(0.64, 0.38), at(0.76, 0.6)];
                g.polyline(&pts, icon, lw);
            }
            Item::Color(i) => {
                let c = st.palette[*i];
                let dia = 0.56 * s;
                let (cx, cy) = (x + (s - dia) / 2.0, y + (s - dia) / 2.0);
                if *i == st.color {
                    g.ellipse(cx - 3.0 * d, cy - 3.0 * d, dia + 6.0 * d, dia + 6.0 * d, argb(0xFFFFFF, 255), 2.0 * d);
                }
                g.fill_ellipse(cx, cy, dia, dia, argb(c, 255));
                g.ellipse(cx, cy, dia, dia, argb(0x000000, 0x50), 1.0);
            }
            Item::Size(i) => {
                let dia = [5.0, 9.0, 14.0][*i] * d;
                if *i == st.size {
                    g.fill_round_rect(x + 2.0, y + 2.0, s - 4.0, s - 4.0, 6.0 * d, argb(0x4A4A4E, 255));
                }
                g.fill_ellipse(x + (s - dia) / 2.0, y + (s - dia) / 2.0, dia, dia, icon);
            }
            Item::Undo | Item::Redo => {
                let m = |fx: f32| if *it == Item::Undo { fx } else { 1.0 - fx };
                let pts = [at(m(0.72), 0.72), at(m(0.72), 0.52), at(m(0.6), 0.4), at(m(0.28), 0.4)];
                g.polyline(&pts, icon, lw);
                g.polyline(&[at(m(0.4), 0.28), at(m(0.28), 0.4), at(m(0.4), 0.52)], icon, lw);
            }
            Item::Copy => {
                g.round_rect(x + 0.38 * s, y + 0.24 * s, 0.36 * s, 0.4 * s, 2.0 * d, icon, lw);
                g.fill_rect(x + 0.24 * s, y + 0.36 * s, 0.38 * s, 0.42 * s, argb(if st.hover == Some(idx) { 0x3F3F42 } else { TOOLBAR_BG }, 255));
                g.round_rect(x + 0.26 * s, y + 0.36 * s, 0.36 * s, 0.4 * s, 2.0 * d, icon, lw);
            }
            Item::Save => {
                g.line(at(0.5, 0.22), at(0.5, 0.58), icon, lw);
                g.polyline(&[at(0.36, 0.46), at(0.5, 0.6), at(0.64, 0.46)], icon, lw);
                g.polyline(&[at(0.25, 0.58), at(0.25, 0.76), at(0.75, 0.76), at(0.75, 0.58)], icon, lw);
            }
            Item::Close => {
                g.line(at(0.33, 0.33), at(0.67, 0.67), icon, lw);
                g.line(at(0.67, 0.33), at(0.33, 0.67), icon, lw);
            }
        }
    }

    // Tooltip under the hovered button.
    if let (Some(idx), Some(font)) = (st.hover, st.ui_font.as_ref()) {
        let (r, it) = st.items[idx];
        let text = tooltip(st, it);
        let (tw, th) = g.measure(&text, font);
        let pad = 6.0 * d;
        let w = tw + 2.0 * pad;
        let x = (r.left as f32).min(cw as f32 - w - 4.0).max(4.0);
        let y = st.tb_h as f32 + 4.0 * d;
        g.fill_round_rect(x, y, w, th + pad, 4.0 * d, argb(0x1C1C1E, 0xF0));
        g.text(&text, x + pad, y + pad / 2.0, font, argb(0xFFFFFF, 255));
    }
}

// ---------------------------------------------------------------------------
// Editing operations
// ---------------------------------------------------------------------------

/// Finish the text being edited. Empty text deletes it.
fn commit_text(st: &mut State) {
    let Some(t) = st.text.take() else { return };
    let shape = (!t.text.trim().is_empty())
        .then(|| Shape::Text { pos: t.pos, text: t.text.clone(), color: t.color, size: t.size });
    if shape != t.original {
        st.undo.push(t.before);
        st.redo.clear();
    }
    if let Some(s) = shape {
        match t.index {
            Some(i) => st.shapes.insert(i.min(st.shapes.len()), s),
            None => st.shapes.push(s),
        }
    }
}

/// Start a new, empty text box at `pos`.
fn begin_new_text(hwnd: HWND, st: &mut State, pos: P) {
    commit_text(st);
    let (color, size) = (current_color(st), font_size(st));
    st.text = Some(TextEdit {
        pos,
        text: String::new(),
        color,
        size,
        caret: 0,
        before: st.shapes.clone(),
        index: None,
        original: None,
    });
    update_caret_timer(hwnd, st);
}

/// Index of the top-most text shape under `p`.
fn text_hit(st: &State, p: P) -> Option<usize> {
    let g = Graphics::from_hdc(st.img.dc);
    st.shapes.iter().enumerate().rev().find_map(|(i, s)| match s {
        Shape::Text { pos, text, size, .. } => {
            let font = text_font(&st.cfg.drawing.font_family, *size)?;
            let l = text_layout(&g, text, &font);
            box_contains(text_box(&l, *pos, *size), p).then_some(i)
        }
        _ => None,
    })
}

/// Re-open text shape `i` for editing, caret at `click` (or at the end).
fn begin_edit(hwnd: HWND, st: &mut State, i: usize, click: Option<P>) {
    st.selected = None;
    st.moving = None;
    let before = st.shapes.clone();
    let shape = st.shapes.remove(i);
    let Shape::Text { pos, text, color, size } = shape.clone() else {
        st.shapes.insert(i, shape);
        return;
    };
    let mut t = TextEdit { pos, caret: text.len(), text, color, size, before, index: Some(i), original: Some(shape) };
    if let (Some(p), Some(font)) = (click, text_font(&st.cfg.drawing.font_family, size)) {
        let g = Graphics::from_hdc(st.img.dc);
        let l = text_layout(&g, &t.text, &font);
        t.caret = caret_at(&g, &t.text, t.pos, &font, &l, p);
    }
    st.text = Some(t);
    st.tool = Tool::Text;
    update_caret_timer(hwnd, st);
}

/// If `p` is inside the text being edited, move the caret there.
fn click_in_edit(st: &mut State, p: P) -> bool {
    let g = Graphics::from_hdc(st.img.dc);
    let family = &st.cfg.drawing.font_family;
    let Some(t) = st.text.as_mut() else { return false };
    let Some(font) = text_font(family, t.size) else { return false };
    let l = text_layout(&g, &t.text, &font);
    if !box_contains(text_box(&l, t.pos, t.size), p) {
        return false;
    }
    t.caret = caret_at(&g, &t.text, t.pos, &font, &l, p);
    true
}

/// Move the caret one line up (-1) or down (+1), keeping its x position.
fn caret_vertical(st: &mut State, dir: i32) {
    let g = Graphics::from_hdc(st.img.dc);
    let family = &st.cfg.drawing.font_family;
    let Some(t) = st.text.as_mut() else { return };
    let Some(font) = text_font(family, t.size) else { return };
    let l = text_layout(&g, &t.text, &font);
    let (line, start) = t.line_start();
    let target = line as i32 + dir;
    if target < 0 || target >= l.widths.len() as i32 {
        return;
    }
    let x = t.pos.x + g.measure(&t.text[start..t.caret], &font).0;
    let y = t.pos.y + l.line_h * (target as f32 + 0.5);
    t.caret = caret_at(&g, &t.text, t.pos, &font, &l, P::new(x, y));
}

fn set_tool(hwnd: HWND, st: &mut State, t: Tool) {
    commit_text(st);
    st.tool = t;
    if t != Tool::Move {
        st.selected = None;
    }
    update_caret_timer(hwnd, st);
}

fn set_color(st: &mut State, i: usize) {
    if i < st.palette.len() {
        st.color = i;
        let c = st.palette[i];
        if let Some(t) = st.text.as_mut() {
            t.color = c;
        }
    }
}

fn set_size(st: &mut State, i: usize) {
    st.size = i.min(2);
    let fs = font_size(st);
    if let Some(t) = st.text.as_mut() {
        t.size = fs;
    }
}

fn undo(st: &mut State) {
    st.selected = None;
    if let Some(t) = st.text.take() {
        st.shapes = t.before; // cancel the text edit in progress
        return;
    }
    if let Some(prev) = st.undo.pop() {
        st.redo.push(std::mem::replace(&mut st.shapes, prev));
    }
}

fn redo(st: &mut State) {
    st.selected = None;
    if let Some(next) = st.redo.pop() {
        st.undo.push(std::mem::replace(&mut st.shapes, next));
    }
}

fn update_caret_timer(hwnd: HWND, st: &mut State) {
    unsafe {
        if st.text.is_some() {
            st.caret_on = true;
            SetTimer(hwnd, CARET_TIMER, GetCaretBlinkTime().clamp(200, 1200), None);
        } else {
            KillTimer(hwnd, CARET_TIMER);
        }
    }
}

/// Hide now, destroy on the next message-loop turn (never free `State`
/// while a handler still holds a reference to it).
fn close(hwnd: HWND, st: &mut State) {
    st.closing = true;
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

fn do_action(hwnd: HWND, st: &mut State, a: Action) {
    match a {
        Action::Close => close(hwnd, st),
        Action::Copy => {
            let img = render_final(st);
            match output::to_clipboard(app::main_hwnd(), &img) {
                Ok(()) => {
                    if st.cfg.notify_on_copy {
                        app::notify("Copied to clipboard", &format!("{} × {} image", img.w, img.h), false);
                    }
                    close(hwnd, st);
                }
                Err(e) => error_box(hwnd, &e),
            }
        }
        Action::Save => {
            let img = render_final(st);
            let dir = std::path::PathBuf::from(&st.cfg.default_location);
            match output::save_png(&dir, &st.cfg.file_name_pattern, &img) {
                Ok(path) => {
                    if st.cfg.copy_to_clipboard_on_save {
                        let _ = output::to_clipboard(app::main_hwnd(), &img);
                    }
                    app::remember_saved(&path);
                    if st.cfg.notify_on_save {
                        app::notify("Screenshot saved", &format!("{}\nClick to show in folder.", path.display()), false);
                    }
                    if st.cfg.close_after_save {
                        close(hwnd, st);
                    } else {
                        invalidate(hwnd);
                    }
                }
                Err(e) => error_box(hwnd, &e),
            }
        }
    }
}

fn click_item(hwnd: HWND, st: &mut State, it: Item) {
    match it {
        Item::Tool(t) => set_tool(hwnd, st, t),
        Item::Color(i) => set_color(st, i),
        Item::Size(i) => set_size(st, i),
        Item::Undo => undo(st),
        Item::Redo => redo(st),
        Item::Copy => return do_action(hwnd, st, Action::Copy),
        Item::Save => return do_action(hwnd, st, Action::Save),
        Item::Close => return do_action(hwnd, st, Action::Close),
    }
    update_caret_timer(hwnd, st);
    invalidate(hwnd);
}

/// Returns true if the key was consumed.
fn on_key(hwnd: HWND, st: &mut State, vk: u16) -> bool {
    let combo = KeyCombo::from_keydown(vk);
    if matches!(vk, VK_CONTROL | VK_SHIFT | VK_MENU | VK_LWIN | VK_RWIN) {
        return false;
    }

    if st.text.is_some() {
        let plain = !combo.ctrl && !combo.alt && !combo.win;
        let fkey = (VK_F1..=VK_F24).contains(&vk);
        // AltGr (= Ctrl+Alt) and Alt+numpad codes type characters; they
        // arrive via WM_CHAR and must not end the text box.
        let altgr = combo.ctrl && combo.alt && !combo.win;
        let alt_code = combo.alt && !combo.ctrl && (VK_NUMPAD0..=VK_NUMPAD9).contains(&vk);
        if (altgr || alt_code) && !fkey {
            return true;
        }
        if plain && !fkey {
            match vk {
                VK_ESCAPE => commit_text(st),
                VK_RETURN if !combo.shift => commit_text(st),
                VK_UP => caret_vertical(st, -1),
                VK_DOWN => caret_vertical(st, 1),
                _ => {
                    let t = st.text.as_mut().unwrap();
                    match vk {
                        VK_RETURN => t.insert("\n"),
                        VK_BACK => {
                            let p = t.prev();
                            t.text.replace_range(p..t.caret, "");
                            t.caret = p;
                        }
                        VK_DELETE => {
                            let n = t.next();
                            t.text.replace_range(t.caret..n, "");
                        }
                        VK_LEFT => t.caret = t.prev(),
                        VK_RIGHT => t.caret = t.next(),
                        VK_HOME => t.caret = t.line_start().1,
                        VK_END => t.caret = t.line_end(),
                        _ => return true, // typed characters arrive via WM_CHAR
                    }
                }
            }
            update_caret_timer(hwnd, st);
            invalidate(hwnd);
            return true;
        }
        if st.keys.undo.matches(&combo) {
            undo(st);
            update_caret_timer(hwnd, st);
            invalidate(hwnd);
            return true;
        }
        // Any other shortcut finishes the text first.
        commit_text(st);
        update_caret_timer(hwnd, st);
    }

    let k = &st.keys;
    if k.close_without_saving.matches(&combo) {
        do_action(hwnd, st, Action::Close);
    } else if k.save_to_default_location.matches(&combo) {
        do_action(hwnd, st, Action::Save);
    } else if k.save_to_clipboard.matches(&combo) {
        do_action(hwnd, st, Action::Copy);
    } else {
        if k.undo.matches(&combo) {
            undo(st);
        } else if k.redo.matches(&combo) {
            redo(st);
        } else if st.selected.is_some() && k.delete_selected.matches(&combo) {
            delete_selected(st);
        } else if st.selected.is_some()
            && matches!(vk, VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN)
            && !combo.ctrl
            && !combo.alt
            && !combo.win
        {
            let step = if combo.shift { 10.0 } else { 1.0 };
            let (dx, dy) = match vk {
                VK_LEFT => (-step, 0.0),
                VK_RIGHT => (step, 0.0),
                VK_UP => (0.0, -step),
                _ => (0.0, step),
            };
            nudge_selected(st, dx, dy);
        } else if k.tool_move.matches(&combo) {
            set_tool(hwnd, st, Tool::Move);
        } else if k.tool_arrow.matches(&combo) {
            set_tool(hwnd, st, Tool::Arrow);
        } else if k.tool_rectangle.matches(&combo) {
            set_tool(hwnd, st, Tool::Rect);
        } else if k.tool_text.matches(&combo) {
            set_tool(hwnd, st, Tool::Text);
        } else if k.tool_pen.matches(&combo) {
            set_tool(hwnd, st, Tool::Pen);
        } else if (0x31..=0x39).contains(&vk) && !combo.ctrl && !combo.alt && !combo.win {
            set_color(st, (vk - 0x31) as usize);
        } else {
            return false;
        }
        invalidate(hwnd);
    }
    true
}

/// Shift-constrain the drag end point: 45° steps for arrows, squares for rects.
fn constrain(tool: Tool, a: P, b: P) -> P {
    let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
    if !shift {
        return b;
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    match tool {
        Tool::Arrow => {
            let len = (dx * dx + dy * dy).sqrt();
            let step = std::f32::consts::FRAC_PI_4;
            let ang = (dy.atan2(dx) / step).round() * step;
            P::new(a.x + len * ang.cos(), a.y + len * ang.sin())
        }
        Tool::Rect => {
            let s = dx.abs().max(dy.abs());
            P::new(a.x + s * dx.signum(), a.y + s * dy.signum())
        }
        _ => b,
    }
}

fn on_mouse_down(hwnd: HWND, st: &mut State, x: i32, y: i32) {
    if y < st.tb_h {
        if let Some(i) = item_at(st, x, y) {
            let it = st.items[i].1;
            click_item(hwnd, st, it);
        }
        return;
    }
    let p = to_image(st, x, y);
    // Click inside the text being edited: just move the caret.
    if click_in_edit(st, p) {
        update_caret_timer(hwnd, st);
        invalidate(hwnd);
        return;
    }
    let (color, width) = (current_color(st), line_width(st));
    match st.tool {
        Tool::Text => {
            commit_text(st);
            match text_hit(st, p) {
                Some(i) => begin_edit(hwnd, st, i, Some(p)),
                None => {
                    let size = font_size(st);
                    begin_new_text(hwnd, st, P::new(p.x, p.y - size * 0.65));
                }
            }
        }
        Tool::Arrow => st.drag = Some(Shape::Arrow { a: p, b: p, color, width }),
        Tool::Rect => st.drag = Some(Shape::Rect { a: p, b: p, color, width }),
        Tool::Pen => st.drag = Some(Shape::Pen { pts: vec![p], color, width }),
        Tool::Move => {
            commit_text(st);
            update_caret_timer(hwnd, st);
            st.selected = shape_hit(st, p);
            if let Some(index) = st.selected {
                st.moving = Some(MoveDrag { index, last: p, before: st.shapes.clone(), moved: false });
                unsafe { SetCapture(hwnd) };
            }
        }
    }
    if st.drag.is_some() {
        commit_text(st);
        update_caret_timer(hwnd, st);
        unsafe { SetCapture(hwnd) };
    }
    invalidate(hwnd);
}

// ---------------------------------------------------------------------------
// Move tool: hit testing, bounds, moving, deleting
// ---------------------------------------------------------------------------

fn seg_dist(p: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(P::new(a.x + dx * t, a.y + dy * t))
}

fn bbox(pts: impl IntoIterator<Item = P>, inflate: f32) -> (f32, f32, f32, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    (x0 - inflate, y0 - inflate, x1 - x0 + 2.0 * inflate, y1 - y0 + 2.0 * inflate)
}

/// Bounding box (x, y, w, h) of a shape in image coordinates.
fn shape_bounds(g: &Graphics, s: &Shape, d: &Drawing) -> (f32, f32, f32, f32) {
    match s {
        Shape::Arrow { a, b, width, .. } => {
            let mut pts = vec![*a, *b];
            if let Some((_, tri)) = arrow_geometry(*a, *b, *width) {
                pts.extend(tri);
            }
            bbox(pts, width / 2.0)
        }
        Shape::Rect { a, b, width, .. } => bbox([*a, *b], width / 2.0),
        Shape::Pen { pts, width, .. } => bbox(pts.iter().copied(), width / 2.0),
        Shape::Text { pos, text, size, .. } => match text_font(&d.font_family, *size) {
            Some(font) => text_box(&text_layout(g, text, &font), *pos, *size),
            None => (pos.x, pos.y, 1.0, 1.0),
        },
    }
}

/// Index of the top-most shape under `p` (lines are hit near their stroke).
fn shape_hit(st: &State, p: P) -> Option<usize> {
    let g = Graphics::from_hdc(st.img.dc);
    let tol = 6.0 / st.scale;
    st.shapes.iter().enumerate().rev().find_map(|(i, s)| {
        let hit = match s {
            Shape::Arrow { a, b, width, .. } => {
                seg_dist(p, *a, *b) <= width / 2.0 + tol
                    || arrow_geometry(*a, *b, *width)
                        .is_some_and(|(_, t)| seg_dist(p, t[1], t[2]).min(seg_dist(p, t[0], t[1])).min(seg_dist(p, t[0], t[2])) <= tol)
            }
            Shape::Rect { a, b, width, .. } => {
                let (c, d) = (P::new(a.x, b.y), P::new(b.x, a.y));
                [(*a, c), (c, *b), (*b, d), (d, *a)].iter().any(|&(u, v)| seg_dist(p, u, v) <= width / 2.0 + tol)
            }
            Shape::Pen { pts, width, .. } => {
                pts.len() == 1 && pts[0].dist(p) <= width / 2.0 + tol
                    || pts.windows(2).any(|w| seg_dist(p, w[0], w[1]) <= width / 2.0 + tol)
            }
            Shape::Text { .. } => box_contains(shape_bounds(&g, s, &st.cfg.drawing), p),
        };
        hit.then_some(i)
    })
}

fn translate(s: &mut Shape, dx: f32, dy: f32) {
    let mv = |p: &mut P| {
        p.x += dx;
        p.y += dy;
    };
    match s {
        Shape::Arrow { a, b, .. } | Shape::Rect { a, b, .. } => {
            mv(a);
            mv(b);
        }
        Shape::Pen { pts, .. } => pts.iter_mut().for_each(mv),
        Shape::Text { pos, .. } => mv(pos),
    }
}

fn nudge_selected(st: &mut State, dx: f32, dy: f32) {
    let Some(i) = st.selected.filter(|&i| i < st.shapes.len()) else { return };
    st.undo.push(st.shapes.clone());
    st.redo.clear();
    translate(&mut st.shapes[i], dx, dy);
}

fn delete_selected(st: &mut State) {
    let Some(i) = st.selected.take().filter(|&i| i < st.shapes.len()) else { return };
    st.undo.push(st.shapes.clone());
    st.redo.clear();
    st.shapes.remove(i);
}

fn on_mouse_move(hwnd: HWND, st: &mut State, x: i32, y: i32) {
    let p = to_image(st, x, y);
    if let Some(m) = st.moving.as_mut() {
        let (dx, dy) = (p.x - m.last.x, p.y - m.last.y);
        if dx != 0.0 || dy != 0.0 {
            if let Some(s) = st.shapes.get_mut(m.index) {
                translate(s, dx, dy);
            }
            m.last = p;
            m.moved = true;
            invalidate(hwnd);
        }
        return;
    }
    let tool = st.tool;
    let min_step = 0.75 / st.scale;
    match st.drag.as_mut() {
        Some(Shape::Arrow { a, b, .. }) | Some(Shape::Rect { a, b, .. }) => *b = constrain(tool, *a, p),
        Some(Shape::Pen { pts, .. }) => {
            if pts.last().is_none_or(|l| l.dist(p) >= min_step) {
                pts.push(p);
            }
        }
        _ => {
            let h = if y < st.tb_h { item_at(st, x, y) } else { None };
            if h != st.hover {
                st.hover = h;
                invalidate(hwnd);
            }
            if !st.tracking {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe { TrackMouseEvent(&mut tme) };
                st.tracking = true;
            }
            return;
        }
    }
    invalidate(hwnd);
}

fn on_mouse_up(st: &mut State) {
    if let Some(m) = st.moving.take() {
        if m.moved {
            st.undo.push(m.before);
            st.redo.clear();
        }
    }
    let Some(s) = st.drag.take() else { return };
    let keep = match &s {
        Shape::Arrow { a, b, .. } => a.dist(*b) >= 3.0,
        Shape::Rect { a, b, .. } => (a.x - b.x).abs() >= 2.0 && (a.y - b.y).abs() >= 2.0,
        _ => true,
    };
    if keep {
        st.undo.push(st.shapes.clone());
        st.redo.clear();
        st.shapes.push(s);
    }
}

fn invalidate(hwnd: HWND) {
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
}

// ---------------------------------------------------------------------------
// Window procedure
// ---------------------------------------------------------------------------

unsafe fn state<'a>(hwnd: HWND) -> Option<&'a mut State> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State).as_mut()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        if let Some(st) = state(hwnd) {
            st.dpi = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            st.tb_h = px(st, 44.0);
        }
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let Some(st) = state(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    if st.closing && msg != WM_CLOSE && msg != WM_DESTROY && msg != WM_NCDESTROY {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd, st);
            0
        }
        WM_SIZE => {
            layout(hwnd, st);
            invalidate(hwnd);
            0
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lp as *mut MINMAXINFO);
            mmi.ptMinTrackSize = POINT { x: toolbar_min_width(st) / 2, y: st.tb_h * 3 };
            0
        }
        WM_DPICHANGED => {
            st.dpi = (wp >> 16 & 0xffff) as f32 / 96.0;
            st.ui_font = None;
            let r = &*(lp as *const RECT);
            SetWindowPos(hwnd, null_mut(), r.left, r.top, rect_w(r), rect_h(r), SWP_NOZORDER | SWP_NOACTIVATE);
            layout(hwnd, st);
            invalidate(hwnd);
            0
        }
        WM_SETCURSOR if (lp & 0xffff) as u32 == HTCLIENT => {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            ScreenToClient(hwnd, &mut pt);
            let cursor = if pt.y < st.tb_h {
                if item_at(st, pt.x, pt.y).is_some() { IDC_HAND } else { IDC_ARROW }
            } else if st.tool == Tool::Text {
                IDC_IBEAM
            } else if st.tool == Tool::Move {
                if st.moving.is_some() || shape_hit(st, to_image(st, pt.x, pt.y)).is_some() {
                    IDC_SIZEALL
                } else {
                    IDC_ARROW
                }
            } else {
                IDC_CROSS
            };
            SetCursor(LoadCursorW(null_mut(), cursor));
            1
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_xy(lp);
            on_mouse_down(hwnd, st, x, y);
            0
        }
        WM_LBUTTONDBLCLK => {
            // Double click on a text with any tool re-opens it for editing.
            let (x, y) = lparam_xy(lp);
            if y >= st.tb_h && st.text.is_none() {
                let p = to_image(st, x, y);
                if let Some(i) = text_hit(st, p) {
                    st.drag = None;
                    st.moving = None;
                    ReleaseCapture();
                    begin_edit(hwnd, st, i, Some(p));
                    invalidate(hwnd);
                    return 0;
                }
            }
            on_mouse_down(hwnd, st, x, y);
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = lparam_xy(lp);
            on_mouse_move(hwnd, st, x, y);
            0
        }
        WM_MOUSELEAVE => {
            st.tracking = false;
            if st.hover.take().is_some() {
                invalidate(hwnd);
            }
            0
        }
        WM_LBUTTONUP => {
            on_mouse_up(st);
            ReleaseCapture();
            invalidate(hwnd);
            0
        }
        WM_CAPTURECHANGED => {
            on_mouse_up(st);
            invalidate(hwnd);
            0
        }
        WM_RBUTTONDOWN => {
            // Right click cancels the shape being drawn.
            if st.drag.take().is_some() {
                ReleaseCapture();
                invalidate(hwnd);
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if on_key(hwnd, st, wp as u16) {
                0
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_SYSCHAR if st.text.is_none() => 0, // no beep / menu for Alt+key
        WM_CHAR => {
            let c = wp as u16;
            if let Some(t) = st.text.as_mut() {
                if (0xD800..0xDC00).contains(&c) {
                    st.high_surrogate = Some(c);
                } else if c >= 0x20 && c != 0x7F {
                    let units: Vec<u16> = match st.high_surrogate.take() {
                        Some(h) if (0xDC00..0xE000).contains(&c) => vec![h, c],
                        _ => vec![c],
                    };
                    t.insert(&String::from_utf16_lossy(&units));
                    st.caret_on = true;
                    invalidate(hwnd);
                }
            }
            0
        }
        WM_TIMER if wp == CARET_TIMER => {
            st.caret_on = !st.caret_on;
            invalidate(hwnd);
            0
        }
        WM_NCDESTROY => {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !p.is_null() {
                drop(Box::from_raw(p));
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
