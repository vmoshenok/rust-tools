import { px } from "../lib/css";
import { useEffect } from "preact/hooks";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, Config } from "../api";
import { getState, loadConfig, onRepoChanged, openTerminal, refreshAll, refreshStatus, setState, ui, updateUi, useAppState } from "../state";
import { toastError, UiHost } from "../ui";
import { Sidebar, addFolderPaths } from "./Sidebar";
import { Toolbar } from "./Toolbar";
import { GraphView } from "./Graph";
import { DetailsPanel } from "./Details";
import { TerminalPanel } from "./TerminalPanel";
import { DiffViewer } from "./DiffViewer";
import { MergeTool } from "./MergeTool";
import { SettingsDialog } from "./Settings";
import { BranchFilterDialog } from "./BranchFilter";
import { RevisionPicker } from "./RevisionPicker";
import { Splitter } from "./Splitter";
import { setTerminalFontSize } from "../terminal";
import { initRepo } from "../actions";

export function App() {
  const s = useAppState();

  useEffect(() => {
    loadConfig().catch(toastError);
    const unlisten = [
      listen<Config>("config-changed", (e) => loadConfig(e.payload).catch(toastError)),
      listen<{ path: string; git: boolean }>("repo-changed", (e) => onRepoChanged(e.payload.path, e.payload.git)),
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type === "drop" && e.payload.paths.length) addFolderPaths(e.payload.paths);
      }),
    ];
    const onFocus = () => refreshStatus();
    const onKey = (e: KeyboardEvent) => {
      const st = getState();
      if (st.diff || st.merge || st.picker) return;
      if (e.ctrlKey && e.key === "`") {
        e.preventDefault();
        openTerminal(!st.terminalOpen);
      } else if (e.key === "F5") {
        e.preventDefault();
        refreshAll();
      } else if (e.ctrlKey && (e.key === "k" || e.key === "K")) {
        e.preventDefault();
        setState({ selected: "WIP", focusCommitRequest: st.focusCommitRequest + 1 });
      }
    };
    window.addEventListener("focus", onFocus);
    window.addEventListener("keydown", onKey);
    // Block the default webview context menu (we show our own).
    const noMenu = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (!t.closest("input, textarea, .xterm")) e.preventDefault();
    };
    window.addEventListener("contextmenu", noMenu);
    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", noMenu);
    };
  }, []);

  useEffect(() => {
    if (s.config) setTerminalFontSize(s.config.settings.terminalFontSize || 13);
  }, [s.config?.settings.terminalFontSize]);

  const u = ui();
  const sidebarW = u.sidebarWidth ?? 270;
  const detailsW = u.detailsWidth ?? 440;
  const termH = u.terminalHeight ?? 260;

  return (
    <div class="app">
      <div class="app-main" style={{ gridTemplateColumns: `${sidebarW}px 4px 1fr` }}>
        <Sidebar />
        <Splitter dir="x" onDrag={(d) => updateUi({ sidebarWidth: Math.max(180, Math.min(600, sidebarW + d)) })} />
        <div class="center">
          <Toolbar />
          <div class="center-body" style={{ gridTemplateColumns: `1fr 4px ${detailsW}px` }}>
            <div class="graph-area">
              {s.repoError ? (
                <div class="empty">
                  <Icon2 />
                  <div class="empty-title">Not a git repository</div>
                  <div class="empty-text">{s.repo}</div>
                  <button class="primary" onClick={initRepo}>
                    Initialize repository (git init)
                  </button>
                  <button onClick={() => s.repo && api.openInExplorer(s.repo)}>Open in Explorer</button>
                </div>
              ) : s.repo ? (
                <GraphView />
              ) : (
                <div class="empty">
                  <Icon2 />
                  <div class="empty-title">No repository selected</div>
                  <div class="empty-text">Add a folder on the left, drop a folder onto the window, or use “Open in Git Ruster” in Explorer.</div>
                </div>
              )}
            </div>
            <Splitter dir="x" onDrag={(d) => updateUi({ detailsWidth: Math.max(300, Math.min(1000, detailsW - d)) })} />
            <DetailsPanel />
          </div>
        </div>
      </div>
      {s.terminalOpen && s.repo && (
        <>
          <Splitter dir="y" onDrag={(d) => updateUi({ terminalHeight: Math.max(100, Math.min(window.innerHeight - 200, termH - d)) })} />
          <div class="terminal-area" style={{ height: px(termH) }}>
            <TerminalPanel />
          </div>
        </>
      )}
      {s.diff && <DiffViewer req={s.diff} />}
      {s.merge && <MergeTool req={s.merge} />}
      {s.settingsOpen && <SettingsDialog />}
      {s.branchFilterOpen && s.info && <BranchFilterDialog />}
      {s.picker && <RevisionPicker req={s.picker} />}
      <UiHost />
    </div>
  );
}

function Icon2() {
  return (
    <svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.2" class="empty-icon">
      <path d="M6 3v12M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a9 9 0 0 1-9 9" />
    </svg>
  );
}
