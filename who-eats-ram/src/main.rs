#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, StyledExt as _, h_flex,
    chart::{AreaChart, PieChart},
    tab::{Tab, TabBar},
    WindowExt as _,
    button::ButtonVariant,
    input::{Input, InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
    table::{Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
use tray_icon::{
    Icon as TrayImage, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const REFRESH: Duration = Duration::from_secs(2);
const PIE_SLICES: usize = 12;
const HISTORY: usize = 120;

// ---------- data ----------

#[derive(Clone)]
struct Proc {
    pid: u32,
    ppid: Option<u32>,
    start: u64,
    name: SharedString,
    ws: u64,      // working set
    private: u64, // private bytes (commit)
    peak_ws: u64,
    peak_private: u64,
    cpu: f32, // % of the whole machine (all cores = 100%)
}

#[derive(Clone, Copy, Default)]
struct Mem {
    ws: u64,
    private: u64,
    peak_ws: u64,
    peak_private: u64,
}

/// Memory of every process from one NtQuerySystemInformation call, keyed by PID.
/// Unlike OpenProcess + GetProcessMemoryInfo (what sysinfo does) it needs no handle, so
/// protected and SYSTEM processes get real numbers too, same as Process Explorer.
fn nt_memory() -> HashMap<u32, Mem> {
    use windows_sys::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessInformation};
    use windows_sys::Win32::System::WindowsProgramming::SYSTEM_PROCESS_INFORMATION;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
    let mut buf: Vec<u64> = vec![0; 64 * 1024]; // u64 keeps the structs 8-byte aligned
    let mut out = HashMap::new();
    // SAFETY: the kernel fills `buf` with a chain of SYSTEM_PROCESS_INFORMATION records linked
    // by NextEntryOffset (0 = last); we only read inside the length it reported.
    unsafe {
        let len = loop {
            let mut needed = 0u32;
            let bytes = (buf.len() * 8) as u32;
            match NtQuerySystemInformation(SystemProcessInformation, buf.as_mut_ptr().cast(), bytes, &mut needed) {
                0 => break needed as usize,
                STATUS_INFO_LENGTH_MISMATCH => buf.resize((needed as usize + 64 * 1024) / 8, 0), // headroom: new processes appear between calls
                _ => return out,
            }
        };
        let base = buf.as_ptr().cast::<u8>();
        let mut off = 0usize;
        while off + size_of::<SYSTEM_PROCESS_INFORMATION>() <= len {
            let e = &*base.add(off).cast::<SYSTEM_PROCESS_INFORMATION>();
            out.insert(e.UniqueProcessId as usize as u32, Mem {
                ws: e.WorkingSetSize as u64,
                private: e.PagefileUsage as u64, // = PrivateUsage from GetProcessMemoryInfo
                peak_ws: e.PeakWorkingSetSize as u64,
                peak_private: e.PeakPagefileUsage as u64,
            });
            if e.NextEntryOffset == 0 {
                break;
            }
            off += e.NextEntryOffset as usize;
        }
    }
    out
}

/// Terminates the given (pid, start time) pairs, skipping any PID that now belongs to a
/// different process. Returns how many could not be killed.
fn kill(targets: &[(u32, u64)]) -> usize {
    let pids: Vec<Pid> = targets.iter().map(|t| Pid::from_u32(t.0)).collect();
    let Ok(mut sys) = System::new() else { return targets.len() };
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, ProcessRefreshKind::nothing());
    targets
        .iter()
        .filter(|&&(pid, start)| {
            sys.process(Pid::from_u32(pid)).is_some_and(|p| p.start_time() == start && !p.kill())
        })
        .count()
}

fn snapshot(sys: &mut System) -> Vec<Proc> {
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    // sysinfo reports per-process CPU relative to one core
    let cores = sys.cpus().len().max(1) as f32;
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu(),
    );
    let mem = nt_memory();
    sys.processes()
        .values()
        .map(|p| {
            let m = mem.get(&p.pid().as_u32()).copied().unwrap_or_default();
            Proc {
            pid: p.pid().as_u32(),
            ppid: p.parent().map(|x| x.as_u32()),
            start: p.start_time(),
            name: p.name().to_string_lossy().into_owned().into(),
            ws: m.ws,
            private: m.private,
            peak_ws: m.peak_ws,
            peak_private: m.peak_private,
            cpu: p.cpu_usage() / cores,
        }})
        .collect()
}

/// Parent must have started before the child (Windows reuses PIDs of dead parents).
/// The tie-break keeps the relation a strict order, so the tree can never cycle.
fn is_older(parent: &Proc, child: &Proc) -> bool {
    (parent.start, parent.pid) < (child.start, child.pid)
}

struct Tree {
    children: Vec<Vec<usize>>,
    parent: Vec<Option<usize>>,
    roots: Vec<usize>,
    /// (working set, private) of the process plus all its descendants.
    totals: Vec<(u64, u64)>,
}

fn build_tree(procs: &[Proc]) -> Tree {
    let index: HashMap<u32, usize> = procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
    let mut children = vec![Vec::new(); procs.len()];
    let mut parent = vec![None; procs.len()];
    let mut roots = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        match p.ppid.and_then(|pp| index.get(&pp)).copied() {
            Some(j) if is_older(&procs[j], p) => {
                children[j].push(i);
                parent[i] = Some(j);
            }
            _ => roots.push(i),
        }
    }
    let mut totals = vec![(0, 0); procs.len()];
    fn acc(i: usize, procs: &[Proc], ch: &[Vec<usize>], t: &mut [(u64, u64)]) -> (u64, u64) {
        let mut s = (procs[i].ws, procs[i].private);
        for &c in &ch[i] {
            let (a, b) = acc(c, procs, ch, t);
            s = (s.0 + a, s.1 + b);
        }
        t[i] = s;
        s
    }
    for &r in &roots {
        acc(r, procs, &children, &mut totals);
    }
    Tree { children, parent, roots, totals }
}

fn fmt_bytes(b: u64) -> String {
    let mb = b as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 { format!("{:.2} GB", mb / 1024.0) } else { format!("{mb:.1} MB") }
}

fn palette(i: usize) -> Hsla {
    hsla((i as f32 * 0.618_034).fract(), 0.62, 0.56, 1.0)
}

// ---------- table (list + tree) ----------

#[derive(Clone, Copy, PartialEq)]
enum Col {
    Name,
    Pid,
    Cpu,
    Ws,
    PeakWs,
    Private,
    PeakPrivate,
    TotalWs,
    TotalPrivate,
}

const LIST_COLS: &[Col] = &[Col::Name, Col::Pid, Col::Cpu, Col::Ws, Col::PeakWs, Col::Private, Col::PeakPrivate];
const TREE_COLS: &[Col] = &[Col::Name, Col::Pid, Col::Cpu, Col::Ws, Col::Private, Col::TotalWs, Col::TotalPrivate];

struct ProcTable {
    procs: Vec<Proc>,
    tree: Tree,
    tree_mode: bool,
    sort_col: Col,
    sort: ColumnSort,
    filter: String, // lowercase
    expanded: HashSet<u32>,
    rows: Vec<(usize, usize)>, // (proc index, depth)
}

impl ProcTable {
    fn new(tree_mode: bool) -> Self {
        Self {
            procs: vec![],
            tree: build_tree(&[]),
            tree_mode,
            sort_col: if tree_mode { Col::TotalWs } else { Col::Ws },
            sort: ColumnSort::Descending,
            filter: String::new(),
            expanded: HashSet::new(),
            rows: vec![],
        }
    }

    fn cols(&self) -> &'static [Col] {
        if self.tree_mode { TREE_COLS } else { LIST_COLS }
    }

    fn set_procs(&mut self, procs: Vec<Proc>) {
        self.tree = build_tree(&procs);
        self.procs = procs;
        self.rebuild_rows();
    }

    fn cmp(&self, a: usize, b: usize) -> Ordering {
        let (p, q) = (&self.procs[a], &self.procs[b]);
        let (ta, tb) = (self.tree.totals[a], self.tree.totals[b]);
        let o = match self.sort_col {
            Col::Name => p.name.to_lowercase().cmp(&q.name.to_lowercase()),
            Col::Pid => p.pid.cmp(&q.pid),
            Col::Cpu => p.cpu.total_cmp(&q.cpu),
            Col::Ws => p.ws.cmp(&q.ws),
            Col::PeakWs => p.peak_ws.cmp(&q.peak_ws),
            Col::Private => p.private.cmp(&q.private),
            Col::PeakPrivate => p.peak_private.cmp(&q.peak_private),
            Col::TotalWs => ta.0.cmp(&tb.0),
            Col::TotalPrivate => ta.1.cmp(&tb.1),
        };
        if self.sort == ColumnSort::Ascending { o } else { o.reverse() }
    }

    /// Which processes to show: search matches, plus (in tree mode) every ancestor of a match.
    fn keep_mask(&self) -> Vec<bool> {
        let f = &self.filter;
        let hits: Vec<bool> = self
            .procs
            .iter()
            .map(|p| f.is_empty() || p.name.to_lowercase().contains(f) || p.pid.to_string().contains(f))
            .collect();
        if !self.tree_mode || f.is_empty() {
            return hits;
        }
        let mut keep = hits.clone();
        for i in (0..hits.len()).filter(|&i| hits[i]) {
            let mut up = self.tree.parent[i];
            while let Some(j) = up {
                if keep[j] {
                    break; // this ancestor chain is already marked
                }
                keep[j] = true;
                up = self.tree.parent[j];
            }
        }
        keep
    }

    fn rebuild_rows(&mut self) {
        let keep = self.keep_mask();
        self.rows.clear();
        if !self.tree_mode {
            let mut all: Vec<usize> = (0..self.procs.len()).filter(|&i| keep[i]).collect();
            all.sort_by(|&a, &b| self.cmp(a, b));
            self.rows = all.into_iter().map(|i| (i, 0)).collect();
            return;
        }
        let searching = !self.filter.is_empty();
        let mut stack: Vec<(usize, usize)> =
            self.sorted(&self.tree.roots, &keep).into_iter().rev().map(|i| (i, 0)).collect();
        while let Some((i, d)) = stack.pop() {
            self.rows.push((i, d));
            if searching || self.expanded.contains(&self.procs[i].pid) {
                stack.extend(self.sorted(&self.tree.children[i], &keep).into_iter().rev().map(|c| (c, d + 1)));
            }
        }
    }

    fn sorted(&self, ix: &[usize], keep: &[bool]) -> Vec<usize> {
        let mut v: Vec<usize> = ix.iter().copied().filter(|&i| keep[i]).collect();
        v.sort_by(|&a, &b| self.cmp(a, b));
        v
    }

    fn pid_at(&self, row: usize) -> Option<u32> {
        self.rows.get(row).map(|&(i, _)| self.procs[i].pid)
    }

    fn name_at(&self, row: usize) -> SharedString {
        self.rows.get(row).map(|&(i, _)| self.procs[i].name.clone()).unwrap_or_default()
    }

    fn row_of(&self, pid: u32) -> Option<usize> {
        self.rows.iter().position(|&(i, _)| self.procs[i].pid == pid)
    }

    /// The process and all its descendants as (pid, start time), children before parents.
    fn subtree(&self, i: usize) -> Vec<(u32, u64)> {
        let mut out = Vec::new();
        let mut stack = vec![(i, false)];
        while let Some((n, done)) = stack.pop() {
            if done {
                out.push((self.procs[n].pid, self.procs[n].start));
            } else {
                stack.push((n, true));
                stack.extend(self.tree.children[n].iter().map(|&c| (c, false)));
            }
        }
        out
    }
}

fn confirm_kill(targets: Vec<(u32, u64)>, title: String, window: &mut Window, cx: &mut App) {
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let targets = targets.clone();
        dialog
            .title(title.clone())
            .description("Unsaved work in these processes will be lost.")
            .show_cancel(true)
            .ok_text("Kill")
            .ok_variant(ButtonVariant::Danger)
            .on_ok(move |_, window, cx| {
                let failed = kill(&targets);
                if failed > 0 {
                    let msg = format!(
                        "{failed} of {} processes could not be killed (access denied? try running as admin)",
                        targets.len()
                    );
                    window.push_notification(SharedString::from(msg), cx);
                }
                true
            })
    });
}

impl TableDelegate for ProcTable {
    fn columns_count(&self, _: &App) -> usize {
        self.cols().len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, ix: usize, _: &App) -> Column {
        let col = self.cols()[ix];
        let (key, name, w) = match col {
            Col::Name => ("name", "Process", 300.),
            Col::Pid => ("pid", "PID", 80.),
            Col::Cpu => ("cpu", "CPU", 80.),
            Col::Ws => ("ws", if self.tree_mode { "Own working set" } else { "Working set" }, 140.),
            Col::PeakWs => ("pws", "Peak working set", 150.),
            Col::Private => ("priv", if self.tree_mode { "Own private" } else { "Private bytes" }, 140.),
            Col::PeakPrivate => ("ppriv", "Peak private", 140.),
            Col::TotalWs => ("tws", "Total working set", 150.),
            Col::TotalPrivate => ("tpriv", "Total private", 150.),
        };
        // no .sortable(): the built-in sort only reacts to a tiny icon; render_th makes the whole header clickable
        let c = Column::new(key, name).width(px(w));
        if col == Col::Name { c } else { c.text_right() }
    }

    fn render_th(&mut self, ix: usize, _: &mut Window, cx: &mut Context<TableState<Self>>) -> impl IntoElement {
        let col = self.cols()[ix];
        let arrow = (col == self.sort_col).then(|| {
            Icon::new(if self.sort == ColumnSort::Ascending { IconName::ArrowUp } else { IconName::ArrowDown })
                .size_3()
                .text_color(cx.theme().primary)
        });
        h_flex()
            .id(("th", ix))
            .w_full()
            .gap_1()
            .cursor_pointer()
            .when(col != Col::Name, |d| d.justify_end())
            .when(col == self.sort_col, |d| d.font_semibold())
            .child(self.column(ix, cx).name)
            .children(arrow)
            .on_click(cx.listener(move |t, _, _, cx| {
                let d = t.delegate_mut();
                d.sort = if d.sort_col == col {
                    if d.sort == ColumnSort::Ascending { ColumnSort::Descending } else { ColumnSort::Ascending }
                } else if col == Col::Name {
                    ColumnSort::Ascending // names read best A→Z, sizes biggest first
                } else {
                    ColumnSort::Descending
                };
                d.sort_col = col;
                d.rebuild_rows();
                cx.notify();
            }))
    }

    fn context_menu(&mut self, row: usize, menu: PopupMenu, _: &mut Window, _: &mut Context<TableState<Self>>) -> PopupMenu {
        let Some(&(i, _)) = self.rows.get(row) else { return menu };
        let p = &self.procs[i];
        let (pid, name) = (p.pid, p.name.clone());
        let one = vec![(pid, p.start)];
        let tree = self.subtree(i);
        let n = tree.len();
        let (n1, n2) = (name.clone(), name.clone());
        menu.item(PopupMenuItem::new("Details…").on_click(move |_, _, cx| open_details(pid, n1.clone(), cx)))
            .separator()
            .item(PopupMenuItem::new("Kill process").on_click(move |_, window, cx| {
                confirm_kill(one.clone(), format!("Kill {n2} (PID {pid})?"), window, cx)
            }))
            .item(PopupMenuItem::new(format!("Kill process tree ({n})")).disabled(n < 2).on_click(
                move |_, window, cx| {
                    let title = format!("Kill {name} (PID {pid}) and {} child processes?", n - 1);
                    confirm_kill(tree.clone(), title, window, cx)
                },
            ))
    }

    fn render_td(&mut self, row: usize, ix: usize, _: &mut Window, cx: &mut Context<TableState<Self>>) -> impl IntoElement {
        let (i, depth) = self.rows[row];
        let p = &self.procs[i];
        let t = self.tree.totals[i];
        let col = self.cols()[ix];
        // 0 = the OS refused to open the process (protected / needs admin)
        let peak = |b: u64| if b == 0 { "—".to_string() } else { fmt_bytes(b) };
        let text: SharedString = match col {
            Col::Name => p.name.clone(),
            Col::Pid => p.pid.to_string().into(),
            Col::Cpu => format!("{:.1}%", p.cpu).into(),
            Col::Ws => fmt_bytes(p.ws).into(),
            Col::PeakWs => peak(p.peak_ws).into(),
            Col::Private => fmt_bytes(p.private).into(),
            Col::PeakPrivate => peak(p.peak_private).into(),
            Col::TotalWs => fmt_bytes(t.0).into(),
            Col::TotalPrivate => fmt_bytes(t.1).into(),
        };
        if col != Col::Name || !self.tree_mode {
            return div().child(text).into_any_element();
        }
        let pid = p.pid;
        let has_kids = !self.tree.children[i].is_empty();
        let open = !self.filter.is_empty() || self.expanded.contains(&pid);
        h_flex()
            .gap_1()
            .pl(px(depth as f32 * 16.))
            .child(
                div()
                    .id(("toggle", pid as usize))
                    .w(px(16.))
                    .text_color(cx.theme().muted_foreground)
                    .when(has_kids, |d| {
                        d.cursor_pointer()
                            .child(if open { "▾" } else { "▸" })
                            .on_click(cx.listener(move |t, _, _, cx| {
                                let d = t.delegate_mut();
                                if !d.expanded.remove(&pid) {
                                    d.expanded.insert(pid);
                                }
                                d.rebuild_rows();
                                cx.stop_propagation();
                                cx.notify();
                            }))
                    }),
            )
            .child(text)
            .when(has_kids, |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("({})", self.tree.children[i].len())),
                )
            })
            .into_any_element()
    }
}

// ---------- tray ----------

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

fn set_main_visible(visible: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_RESTORE, SetForegroundWindow, ShowWindow};
    let h = MAIN_HWND.load(Relaxed) as windows_sys::Win32::Foundation::HWND;
    if h.is_null() {
        return;
    }
    // SAFETY: h is the main window's HWND, which lives for the whole app.
    unsafe {
        if visible {
            ShowWindow(h, SW_RESTORE);
            SetForegroundWindow(h);
        } else {
            ShowWindow(h, SW_HIDE);
        }
    }
}

/// 32×32 icon with two bars filling from the bottom: RAM (blue, red above 85%) and CPU (green).
fn tray_pixels(ram: f32, cpu: f32) -> Vec<u8> {
    const S: usize = 32;
    let mut px = vec![0u8; S * S * 4];
    let mut bar = |x0: usize, frac: f32, rgb: [u8; 3]| {
        let filled = (frac.clamp(0., 1.) * (S - 2) as f32).round() as usize;
        for y in 1..S - 1 {
            let on = y >= S - 1 - filled;
            let c = if on { [rgb[0], rgb[1], rgb[2], 255] } else { [90, 90, 90, 160] };
            for x in x0..x0 + 13 {
                px[(y * S + x) * 4..][..4].copy_from_slice(&c);
            }
        }
    };
    bar(2, ram, if ram > 0.85 { [235, 70, 70] } else { [70, 145, 245] });
    bar(17, cpu, [70, 205, 120]);
    px
}

fn tray_image(ram: f32, cpu: f32) -> Option<TrayImage> {
    TrayImage::from_rgba(tray_pixels(ram, cpu), 32, 32).ok()
}

// ---------- main window ----------

#[derive(Clone, Copy, PartialEq)]
enum View {
    List,
    Tree,
    Pie,
}

struct MainView {
    sys: System,
    view: View,
    pie_private: bool,
    procs: Vec<Proc>,
    search: Entity<InputState>,
    list: Entity<TableState<ProcTable>>,
    tree: Entity<TableState<ProcTable>>,
    tray: Option<TrayIcon>,
    _task: Task<()>,
    _tray_task: Task<()>,
}

impl MainView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| TableState::new(ProcTable::new(false), window, cx).col_movable(false).sortable(false));
        let tree = cx.new(|cx| TableState::new(ProcTable::new(true), window, cx).col_movable(false).sortable(false));
        for t in [&list, &tree] {
            cx.subscribe(t, |_, t, ev: &TableEvent, cx| {
                if let TableEvent::DoubleClickedRow(row) = ev {
                    let d = t.read(cx).delegate();
                    if let Some(pid) = d.pid_at(*row) {
                        let name = d.name_at(*row);
                        open_details(pid, name, cx);
                    }
                }
            })
            .detach();
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search name or PID…"));
        cx.subscribe(&search, |this, s, ev: &InputEvent, cx| {
            if !matches!(ev, InputEvent::Change) {
                return;
            }
            let f = s.read(cx).value().to_lowercase();
            for t in [&this.list, &this.tree] {
                t.update(cx, |t, cx| {
                    let d = t.delegate_mut();
                    d.filter = f.clone();
                    d.rebuild_rows();
                    cx.notify();
                });
            }
        })
        .detach();
        if let Ok(h) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::Win32(w) = h.as_raw()
        {
            MAIN_HWND.store(w.hwnd.get(), Relaxed);
        }
        let (show, close) = (MenuItem::new("Show", true, None), MenuItem::new("Close", true, None));
        let (show_id, close_id) = (show.id().clone(), close.id().clone());
        let tray = Menu::with_items(&[&show, &close]).ok().and_then(|menu| {
            let b = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Who eats my RAM").with_menu_on_left_click(false);
            b.with_icon(tray_image(0., 0.)?).build().ok()
        });
        // without a tray icon a hidden window could never come back, so only then hide instead of close
        if tray.is_some() {
            window.on_window_should_close(cx, |_, _| {
                set_main_visible(false);
                false
            });
        }
        let tray_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(150)).await;
                while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        set_main_visible(true);
                    }
                }
                while let Ok(ev) = MenuEvent::receiver().try_recv() {
                    if ev.id == show_id {
                        set_main_visible(true);
                    } else if ev.id == close_id {
                        let _ = this.update(cx, |this, cx| {
                            this.tray.take(); // remove the icon now, not after the process is gone
                            cx.quit();
                        });
                        return;
                    }
                }
            }
        });
        let task = cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(REFRESH).await;
            }
        });
        Self { sys: System::new().expect("sysinfo init"), view: View::List, pie_private: false, procs: vec![], search, list, tree, tray, _task: task, _tray_task: tray_task }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.procs = snapshot(&mut self.sys);
        if let Some(tray) = &self.tray {
            let (used, total) = (self.sys.used_memory(), self.sys.total_memory().max(1));
            let (ram, cpu) = (used as f32 / total as f32, self.sys.global_cpu_usage());
            let _ = tray.set_icon(tray_image(ram, cpu / 100.));
            let _ = tray.set_tooltip(Some(format!(
                "Who eats my RAM
RAM {} / {} ({:.0}%)
CPU {cpu:.0}%",
                fmt_bytes(used),
                fmt_bytes(total),
                ram * 100.
            )));
        }
        for t in [&self.list, &self.tree] {
            let procs = self.procs.clone();
            t.update(cx, |t, cx| {
                // keep the selection on the same process after re-sorting
                let sel = t.selected_row().and_then(|r| t.delegate().pid_at(r));
                t.delegate_mut().set_procs(procs);
                if let Some(r) = sel.and_then(|p| t.delegate().row_of(p)) {
                    t.set_selected_row(r, cx);
                }
                cx.notify();
            });
        }
        cx.notify();
    }

    fn render_pie(&self, cx: &mut Context<Self>) -> AnyElement {
        let private = self.pie_private;
        let mut groups: HashMap<SharedString, (u64, usize)> = HashMap::new();
        for p in &self.procs {
            let e = groups.entry(p.name.clone()).or_default();
            e.0 += if private { p.private } else { p.ws };
            e.1 += 1;
        }
        let mut slices: Vec<(SharedString, u64, usize)> = groups.into_iter().map(|(n, (b, c))| (n, b, c)).collect();
        slices.sort_by(|a, b| b.1.cmp(&a.1));
        if slices.len() > PIE_SLICES {
            let rest = slices.split_off(PIE_SLICES);
            slices.push(("Other".into(), rest.iter().map(|s| s.1).sum(), rest.iter().map(|s| s.2).sum()));
        }
        let total: u64 = slices.iter().map(|s| s.1).sum::<u64>().max(1);
        let data: Vec<(usize, SharedString, u64)> = slices.iter().enumerate().map(|(i, s)| (i, s.0.clone(), s.1)).collect();
        let theme = cx.theme();

        let legend = v_flex().gap_1().min_w(px(320.)).children(slices.iter().enumerate().map(|(i, (name, bytes, count))| {
            h_flex()
                .gap_2()
                .child(div().size(px(12.)).rounded_sm().bg(palette(i)))
                .child(div().flex_1().child(if *count > 1 { format!("{name} ×{count}") } else { name.to_string() }))
                .child(div().text_color(theme.muted_foreground).child(format!("{:.1}%", *bytes as f64 * 100. / total as f64)))
                .child(div().w(px(90.)).text_right().child(fmt_bytes(*bytes)))
        }));

        h_flex()
            .size_full()
            .p_4()
            .gap_6()
            .child(
                div().flex_1().h_full().child(
                    PieChart::new(data)
                        .appear_key(private)
                        .inner_radius_ratio(0.55)
                        .pad_angle(0.01)
                        .value(|d: &(usize, SharedString, u64)| d.2 as f32)
                        .color(|d: &(usize, SharedString, u64)| palette(d.0))
                        .label(|d: &(usize, SharedString, u64)| d.1.clone())
                        .tooltip_name(|d: &(usize, SharedString, u64)| d.1.clone())
                        .tooltip_value(|d: &(usize, SharedString, u64), _, _| fmt_bytes(d.2).into()),
                ),
            )
            .child(legend)
            .into_any_element()
    }
}

impl Render for MainView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let used = self.sys.used_memory();
        let total = self.sys.total_memory().max(1);
        let pct = used as f32 / total as f32;
        let theme = cx.theme().clone();

        let tabs = TabBar::new("views")
            .segmented()
            .selected_index(self.view as usize)
            .child(Tab::new().label("List"))
            .child(Tab::new().label("Tree"))
            .child(Tab::new().label("Pie chart"))
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                this.view = [View::List, View::Tree, View::Pie][*ix];
                cx.notify();
            }));

        let body = match self.view {
            View::List => DataTable::new(&self.list).stripe(true).into_any_element(),
            View::Tree => DataTable::new(&self.tree).stripe(true).into_any_element(),
            View::Pie => v_flex()
                .size_full()
                .child(
                    h_flex().px_4().pt_3().child(
                        TabBar::new("pie-metric")
                            .segmented()
                            .selected_index(self.pie_private as usize)
                            .child(Tab::new().label("Working set"))
                            .child(Tab::new().label("Private bytes"))
                            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                this.pie_private = *ix == 1;
                                cx.notify();
                            })),
                    ),
                )
                .child(div().flex_1().child(self.render_pie(cx)))
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                h_flex()
                    .p_3()
                    .gap_4()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_lg().font_bold().child("Who eats my RAM"))
                    .child(tabs)
                    .when(self.view != View::Pie, |d| d.child(Input::new(&self.search).cleanable(true).w(px(260.))))
                    .child(div().flex_1())
                    .child(format!("{} processes · CPU {:.0}%", self.procs.len(), self.sys.global_cpu_usage()))
                    .child(
                        v_flex()
                            .w(px(240.))
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .child(format!("RAM {} / {} ({:.0}%)", fmt_bytes(used), fmt_bytes(total), pct * 100.)),
                            )
                            .child(
                                div().h(px(6.)).w_full().rounded_full().bg(theme.secondary).child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .w(relative(pct))
                                        .bg(if pct > 0.85 { theme.danger } else { theme.primary }),
                                ),
                            ),
                    ),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}

// ---------- details window ----------

#[derive(Clone)]
struct Sample {
    t: String,
    ws: f64,
    private: f64,
}

struct Details {
    pid: Pid,
    sys: System,
    exe: String,
    cwd: String,
    cmd: Vec<String>,
    env: Vec<String>,
    alive: bool,
    peak: (u64, u64),
    history: VecDeque<Sample>,
    tick: u64,
    _task: Task<()>,
}

impl Details {
    fn new(pid: u32, cx: &mut Context<Self>) -> Self {
        let pid = Pid::from_u32(pid);
        let mut sys = System::new().expect("sysinfo init");
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .with_environ(UpdateKind::Always)
                .with_exe(UpdateKind::Always)
                .with_cwd(UpdateKind::Always),
        );
        let s = |o: &std::ffi::OsString| o.to_string_lossy().into_owned();
        let p = sys.process(pid);
        let exe = p.and_then(|p| p.exe()).map(|e| e.display().to_string()).unwrap_or_default();
        let cwd = p.and_then(|p| p.cwd()).map(|e| e.display().to_string()).unwrap_or_default();
        let cmd = p.map(|p| p.cmd().iter().map(s).collect()).unwrap_or_default();
        let mut env: Vec<String> = p.map(|p| p.environ().iter().map(s).collect()).unwrap_or_default();
        env.sort_by_key(|e| e.to_lowercase());
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |this, cx| this.sample(cx)).is_err() {
                    break;
                }
            }
        });
        let mut me = Self { pid, sys, exe, cwd, cmd, env, alive: true, peak: (0, 0), history: VecDeque::new(), tick: 0, _task: task };
        me.push_sample();
        me
    }

    fn push_sample(&mut self) {
        if self.sys.process(self.pid).is_none() {
            self.alive = false;
            return;
        }
        let m = nt_memory().get(&self.pid.as_u32()).copied().unwrap_or_default();
        self.peak = (m.peak_ws, m.peak_private);
        let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
        self.history.push_back(Sample { t: format!("{}s", self.tick), ws: mb(m.ws), private: mb(m.private) });
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }
        self.tick += 1;
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        if !self.alive {
            return;
        }
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        self.push_sample();
        cx.notify();
    }
}

fn section(title: &'static str, theme: &gpui_kit::component::Theme) -> Div {
    v_flex()
        .gap_1()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .child(div().text_sm().font_semibold().text_color(theme.muted_foreground).child(title))
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let last = self.history.back().cloned();
        let (min, max) = self.history.iter().fold((f64::MAX, 0f64), |(lo, hi), s| (lo.min(s.ws), hi.max(s.ws)));
        let stat = |label: &str, v: String| v_flex().child(div().text_xs().text_color(theme.muted_foreground).child(label.to_string())).child(div().text_lg().font_semibold().child(v));
        let mb = |v: f64| fmt_bytes((v * 1024.0 * 1024.0) as u64);
        let data: Vec<Sample> = self.history.iter().cloned().collect();
        let empty = || "(not available — access denied or empty)".to_string();

        v_flex()
            .id("details")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(
                h_flex()
                    .gap_6()
                    .child(stat("Status", if self.alive { "running".into() } else { "exited".into() }))
                    .child(stat("Working set", last.as_ref().map_or("-".into(), |s| mb(s.ws))))
                    .child(stat("Private bytes", last.as_ref().map_or("-".into(), |s| mb(s.private))))
                    .child(stat("Peak working set", if self.peak.0 == 0 { "-".into() } else { fmt_bytes(self.peak.0) }))
                    .child(stat("Peak private", if self.peak.1 == 0 { "-".into() } else { fmt_bytes(self.peak.1) }))
                    .child(stat("Min / max WS (window)", if data.is_empty() { "-".into() } else { format!("{} / {}", mb(min), mb(max)) })),
            )
            .child(
                section("Memory over time (MB) — working set / private", &theme).child(
                    div().h(px(240.)).child(
                        AreaChart::new(data)
                            .x(|s: &Sample| s.t.clone())
                            .y(|s: &Sample| s.ws)
                            .stroke(theme.chart_1)
                            .fill(theme.chart_1.opacity(0.25))
                            .y(|s: &Sample| s.private)
                            .stroke(theme.chart_2)
                            .fill(theme.chart_2.opacity(0.15))
                            .tick_margin(15)
                            .y_axis(true)
                            .grid(true),
                    ),
                ),
            )
            .child(section("Executable", &theme).child(if self.exe.is_empty() { empty() } else { self.exe.clone() }))
            .child(section("Working directory", &theme).child(if self.cwd.is_empty() { empty() } else { self.cwd.clone() }))
            .child(section("Command line", &theme).child(if self.cmd.is_empty() { empty() } else { self.cmd.join(" ") }))
            .child(
                section("Parameters", &theme).children(if self.cmd.len() > 1 {
                    self.cmd[1..].iter().enumerate().map(|(i, a)| format!("[{}] {a}", i + 1)).collect()
                } else {
                    vec!["(none)".to_string()]
                }),
            )
            .child(section("Environment variables", &theme).children(if self.env.is_empty() {
                vec![div().child(empty())]
            } else {
                self.env
                    .iter()
                    .map(|e| {
                        let (k, v) = e.split_once('=').unwrap_or((e, ""));
                        h_flex()
                            .items_start()
                            .gap_2()
                            .child(div().w(px(220.)).flex_none().font_semibold().child(k.to_string()))
                            .child(div().flex_1().child(v.to_string()))
                    })
                    .collect()
            }))
    }
}

fn open_details(pid: u32, name: SharedString, cx: &mut App) {
    let opts = WindowOptions {
        titlebar: Some(TitlebarOptions { title: Some(format!("{name} (PID {pid})").into()), ..Default::default() }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(900.), px(760.)), cx))),
        ..Default::default()
    };
    if let Err(e) = gpui_kit::open_window(opts, cx, |_, cx| cx.new(|cx| Details::new(pid, cx))) {
        eprintln!("failed to open details window: {e}");
    }
}

fn main() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        let opts = WindowOptions {
            titlebar: Some(TitlebarOptions { title: Some("Who eats my RAM".into()), ..Default::default() }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1200.), px(780.)), cx))),
            ..Default::default()
        };
        gpui_kit::open_window(opts, cx, |window, cx| cx.new(|cx| MainView::new(window, cx))).expect("failed to open window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: Option<u32>, start: u64, ws: u64) -> Proc {
        Proc { pid, ppid, start, name: format!("p{pid}").into(), ws, private: ws * 2, peak_ws: 0, peak_private: 0, cpu: 0.0 }
    }

    #[core::prelude::v1::test]
    fn tree_totals_and_pid_reuse() {
        let procs = vec![
            p(1, None, 0, 10),
            p(2, Some(1), 1, 5),
            p(3, Some(2), 2, 1),
            p(4, Some(9), 3, 7),  // parent missing -> root
            p(5, Some(6), 4, 2),  // parent 6 started later (reused PID) -> root
            p(6, Some(5), 5, 3),  // 5 is a valid parent of 6
            p(7, Some(8), 6, 1),  // same start time pair: must not cycle
            p(8, Some(7), 6, 1),
        ];
        let t = build_tree(&procs);
        assert_eq!(t.totals[0], (16, 32));
        assert_eq!(t.totals[1], (6, 12));
        assert_eq!(t.totals[4], (5, 10));
        let roots: Vec<u32> = t.roots.iter().map(|&i| procs[i].pid).collect();
        assert_eq!(roots, vec![1, 4, 5, 7]);
        let sum: u64 = t.roots.iter().map(|&r| t.totals[r].0).sum();
        assert_eq!(sum, procs.iter().map(|p| p.ws).sum::<u64>());
    }

    fn sample() -> Vec<Proc> {
        let mut v = vec![p(1, None, 0, 10), p(2, Some(1), 1, 5), p(3, Some(2), 2, 1), p(4, None, 3, 7)];
        v[2].name = "needle.exe".into();
        v
    }

    #[core::prelude::v1::test]
    fn tree_search_keeps_ancestors_and_subtree_kills_children_first() {
        let mut t = ProcTable::new(true);
        t.set_procs(sample());
        t.filter = "needle".into();
        t.rebuild_rows();
        let pids: Vec<u32> = t.rows.iter().map(|&(i, _)| t.procs[i].pid).collect();
        assert_eq!(pids, vec![1, 2, 3]);
        let order: Vec<u32> = t.subtree(0).iter().map(|x| x.0).collect();
        assert_eq!(order, vec![3, 2, 1]);
    }

    #[core::prelude::v1::test]
    fn nt_memory_sees_self_and_system() {
        let m = nt_memory();
        assert!(m[&std::process::id()].ws > 0 && m[&std::process::id()].private > 0);
        assert!(m.contains_key(&4), "System process needs no handle"); // fails via OpenProcess without admin
    }

    #[core::prelude::v1::test]
    fn kill_checks_start_time_then_terminates() {
        let mut child = std::process::Command::new("ping").args(["-n", "60", "127.0.0.1"]).spawn().unwrap();
        let pid = child.id();
        let mut sys = System::new().unwrap();
        sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
        let start = sys.process(Pid::from_u32(pid)).unwrap().start_time();
        assert_eq!(kill(&[(pid, start + 1000)]), 0); // looks like a reused PID: skipped
        assert!(child.try_wait().unwrap().is_none(), "must not kill a different process");
        assert_eq!(kill(&[(pid, start)]), 0);
        assert!(child.wait().is_ok());
    }

    #[core::prelude::v1::test]
    fn tray_bars_fill_from_bottom() {
        let px = tray_pixels(0.5, 0.0);
        let alpha = |x: usize, y: usize| px[(y * 32 + x) * 4 + 3];
        assert_eq!(alpha(5, 30), 255); // RAM bar bottom filled
        assert_eq!(alpha(5, 2), 160); // RAM bar top empty track
        assert_eq!(alpha(20, 30), 160); // CPU 0% -> empty
        assert_eq!(alpha(0, 0), 0); // transparent margin
    }

    #[core::prelude::v1::test]
    fn list_search_shows_only_matches() {
        let mut t = ProcTable::new(false);
        t.set_procs(sample());
        t.filter = "4".into(); // matches PID 4 only
        t.rebuild_rows();
        assert_eq!(t.rows.len(), 1);
    }
}
