// xterm.js instances, one per repository, kept alive while the app runs.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";

interface Entry {
  term: Terminal;
  fit: FitAddon;
  el: HTMLDivElement;
  cwd: string;
  opened: boolean;
  exited: boolean;
}

const entries = new Map<string, Entry>();
const pending = new Map<string, string[]>();

listen<{ id: string; data: string }>("term-data", (e) => {
  const t = entries.get(e.payload.id);
  if (t?.opened) t.term.write(e.payload.data);
  else {
    const buf = pending.get(e.payload.id) ?? [];
    buf.push(e.payload.data);
    pending.set(e.payload.id, buf);
  }
});

listen<{ id: string }>("term-exit", (e) => {
  const t = entries.get(e.payload.id);
  if (!t) return;
  t.exited = true;
  t.term.write("\r\n\x1b[90m[shell exited — press Enter to start a new one]\x1b[0m\r\n");
});

const theme = {
  background: "#1b1c1f",
  foreground: "#d4d6da",
  cursor: "#d4d6da",
  selectionBackground: "#2f4a7a",
  black: "#1e1f22",
  red: "#f75464",
  green: "#5fb865",
  yellow: "#e5c07b",
  blue: "#548af7",
  magenta: "#c678dd",
  cyan: "#56b6c2",
  white: "#d4d6da",
  brightBlack: "#6f737a",
  brightRed: "#ff7b85",
  brightGreen: "#89d185",
  brightYellow: "#f0d48a",
  brightBlue: "#7aa4ff",
  brightMagenta: "#d896ec",
  brightCyan: "#7fd3dd",
  brightWhite: "#ffffff",
};

let fontSize = 13;
export function setTerminalFontSize(size: number) {
  fontSize = size;
  entries.forEach((t) => {
    t.term.options.fontSize = size;
    fit(t);
  });
}

function fit(t: Entry) {
  if (!t.opened || !t.el.isConnected || t.el.clientWidth < 20) return;
  try {
    t.fit.fit();
    api.termResize(idOf(t), t.term.cols, t.term.rows).catch(() => {});
  } catch {
    /* not visible */
  }
}

function idOf(t: Entry) {
  for (const [k, v] of entries) if (v === t) return k;
  return "";
}

function start(id: string, t: Entry) {
  t.exited = false;
  api.termOpen(id, t.cwd, t.term.cols, t.term.rows).catch((e) => t.term.write(`\r\n\x1b[31m${e}\x1b[0m\r\n`));
}

function create(id: string, cwd: string): Entry {
  const term = new Terminal({
    fontFamily: "'Cascadia Mono', 'JetBrains Mono', Consolas, monospace",
    fontSize,
    cursorBlink: true,
    scrollback: 10000,
    allowProposedApi: true,
    theme,
  });
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  const el = document.createElement("div");
  el.className = "xterm-host";
  const entry: Entry = { term, fit: fitAddon, el, cwd, opened: false, exited: false };

  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown") return true;
    if (e.ctrlKey && e.key === "c" && term.hasSelection()) {
      navigator.clipboard.writeText(term.getSelection()).catch(() => {});
      term.clearSelection();
      return false;
    }
    if (e.ctrlKey && (e.key === "v" || e.key === "V")) {
      e.preventDefault();
      navigator.clipboard.readText().then((text) => void (text && api.termWrite(id, text))).catch(() => {});
      return false;
    }
    // Let app shortcuts through.
    if (e.ctrlKey && e.key === "`") return false;
    return true;
  });
  term.onData((data) => {
    if (entry.exited) {
      if (data === "\r") start(id, entry);
      return;
    }
    api.termWrite(id, data).catch(() => {});
  });
  entries.set(id, entry);
  return entry;
}

/** Attaches the terminal of `id` into `host` (creating and starting it on first use). */
export function attachTerminal(id: string, cwd: string, host: HTMLElement) {
  const t = entries.get(id) ?? create(id, cwd);
  host.querySelectorAll(".xterm-host").forEach((el) => el !== t.el && el.remove());
  if (t.el.parentElement !== host) host.appendChild(t.el);
  if (!t.opened) {
    t.term.open(t.el);
    t.opened = true;
    const buf = pending.get(id);
    if (buf) {
      buf.forEach((d) => t.term.write(d));
      pending.delete(id);
    }
    requestAnimationFrame(() => {
      fit(t);
      start(id, t);
    });
  } else {
    requestAnimationFrame(() => fit(t));
  }
  return t;
}

export function fitTerminal(id: string) {
  const t = entries.get(id);
  if (t) fit(t);
}

export function focusTerminal(id: string) {
  entries.get(id)?.term.focus();
}

export function clearTerminal(id: string) {
  entries.get(id)?.term.clear();
}

export async function restartTerminal(id: string) {
  const t = entries.get(id);
  if (!t) return;
  await api.termClose(id).catch(() => {});
  t.term.reset();
  start(id, t);
}

/** Marks the terminal as running (term_run may have started the shell itself). */
export function markRunning(id: string) {
  const t = entries.get(id);
  if (t) t.exited = false;
}
