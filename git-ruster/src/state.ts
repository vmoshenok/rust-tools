// Application store: one observable object, components re-render on change.
import { useEffect, useState } from "preact/hooks";
import { api, Config, FileChange, FolderNode, GraphData, RepoInfo, RepoView, UiState } from "./api";
import { toastError } from "./ui";

export type Side =
  | { kind: "rev"; rev: string; path: string }
  | { kind: "index"; path: string }
  | { kind: "work"; path: string }
  | { kind: "empty" };

export interface DiffFile {
  path: string;
  oldPath?: string | null;
  status: string;
  left: Side;
  right: Side;
  leftLabel: string;
  rightLabel: string;
}

export interface DiffRequest {
  title: string;
  files: DiffFile[];
  index: number;
}

export interface MergeRequest {
  path: string;
}

export interface Revision {
  rev: string;
  label: string;
}

export interface RevisionPickerRequest {
  title: string;
  resolve: (r: Revision | null) => void;
}

export interface AppState {
  config: Config | null;
  folders: FolderNode[];
  repo: string | null;
  info: RepoInfo | null;
  graph: GraphData | null;
  status: FileChange[];
  /** selected commit sha, "WIP" or null */
  selected: string | null;
  /** second commit marked with Ctrl+click (for "Show diff" between two commits) */
  compare: string | null;
  /** open revision picker ("Show diff with…") */
  picker: RevisionPickerRequest | null;
  loading: boolean;
  /** shown instead of the graph, e.g. "not a git repository" */
  repoError: string | null;
  terminalOpen: boolean;
  diff: DiffRequest | null;
  merge: MergeRequest | null;
  settingsOpen: boolean;
  branchFilterOpen: boolean;
  /** bumped to ask the commit panel to switch amend on */
  amendRequest: number;
  /** bumped to focus the commit message box */
  focusCommitRequest: number;
  /** sha to scroll the graph to */
  scrollTo: { sha: string; n: number } | null;
}

let state: AppState = {
  config: null,
  folders: [],
  repo: null,
  info: null,
  graph: null,
  status: [],
  selected: null,
  compare: null,
  picker: null,
  loading: false,
  repoError: null,
  terminalOpen: false,
  diff: null,
  merge: null,
  settingsOpen: false,
  branchFilterOpen: false,
  amendRequest: 0,
  focusCommitRequest: 0,
  scrollTo: null,
};

const listeners = new Set<() => void>();

export function getState() {
  return state;
}

export function setState(patch: Partial<AppState>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

export function useAppState(): AppState {
  const [, force] = useState(0);
  useEffect(() => {
    const l = () => force((n) => n + 1);
    listeners.add(l);
    return () => void listeners.delete(l);
  }, []);
  return state;
}

// ---------------------------------------------------------------------------------------------
// UI state persistence (panel sizes etc.)
// ---------------------------------------------------------------------------------------------

let uiSaveTimer: number | undefined;
export function ui(): UiState {
  return state.config?.ui ?? {};
}
export function updateUi(patch: Partial<UiState>) {
  if (!state.config) return;
  const next = { ...ui(), ...patch };
  setState({ config: { ...state.config, ui: next } });
  clearTimeout(uiSaveTimer);
  uiSaveTimer = window.setTimeout(() => api.saveUi(next).catch(() => {}), 500);
}

// ---------------------------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------------------------

export async function loadConfig(cfg?: Config) {
  const config = cfg ?? (await api.getConfig());
  setState({ config, terminalOpen: config.ui?.terminalOpen ?? false });
  await reloadFolders();
  if (config.selected && config.selected !== state.repo) await selectRepo(config.selected);
  else if (!config.selected && !state.repo) {
    const first = state.folders.find((f) => f.kind === "repo")?.path ?? state.folders.find((f) => f.kind === "group")?.children[0]?.path;
    if (first) await selectRepo(first);
  }
}

export async function reloadFolders() {
  try {
    setState({ folders: await api.listFolders() });
  } catch (e) {
    toastError(e);
  }
}

export async function selectRepo(path: string | null) {
  if (path === state.repo && path) return refreshAll();
  setState({ repo: path, info: null, graph: null, status: [], selected: null, compare: null, repoError: null, diff: null, merge: null });
  api.setSelected(path).catch(() => {});
  api.watchRepo(path).catch(() => {});
  if (path) await refreshAll();
}

/** Message prefix of stashes created by smart checkout / smart pull (see actions.ts). */
export const SMART_STASH_PREFIX = "git-ruster smart";

export function hasSmartStash(info: RepoInfo | null) {
  return !!info?.stashes[0]?.message.includes(SMART_STASH_PREFIX);
}

/** The graph gets a "// WIP" row for local changes, in-progress operations and pending smart-checkout stashes. */
function wantWip(info: RepoInfo | null, status: FileChange[]) {
  return status.length > 0 || (!!info && info.state !== "none") || hasSmartStash(info);
}

/** Labels for the two sides of a conflict. */
export function conflictLabels(): { ours: string; theirs: string } {
  const info = state.info;
  if (info?.oursLabel) return { ours: info.oursLabel, theirs: info.theirsLabel };
  if (hasSmartStash(info)) {
    const pulled = info!.stashes[0].message.includes(`${SMART_STASH_PREFIX} pull`);
    return { ours: `${pulled ? "Pulled" : "Checked out"} (${info?.branch ?? "HEAD"})`, theirs: "Your stashed changes" };
  }
  return { ours: `Yours (${info?.branch ?? "HEAD"})`, theirs: "Theirs" };
}

let refreshSeq = 0;
export async function refreshAll() {
  const repo = state.repo;
  if (!repo) return;
  const seq = ++refreshSeq;
  setState({ loading: true });
  try {
    const [info, status] = await Promise.all([api.repoInfo(repo), api.status(repo)]);
    const graph = await api.graph(repo, state.config?.settings.maxCommits ?? 3000, wantWip(info, status));
    if (seq !== refreshSeq || state.repo !== repo) return;
    let selected = state.selected;
    const exists = (sha: string | null) => !!sha && (sha === "WIP" || graph.rows.some((r) => r.sha === sha) || info.stashes.some((s) => s.sha === sha));
    if (!exists(selected)) selected = wantWip(info, status) ? "WIP" : info.headSha ?? graph.rows[0]?.sha ?? "WIP";
    setState({ info, status, graph, selected, repoError: null, loading: false });
  } catch (e) {
    if (seq !== refreshSeq || state.repo !== repo) return;
    setState({ info: null, graph: null, status: [], repoError: String(e), loading: false });
  }
}

export async function refreshStatus() {
  const repo = state.repo;
  if (!repo || !state.graph) return;
  try {
    const status = await api.status(repo);
    if (state.repo !== repo) return;
    const hadWip = state.graph?.rows[0]?.wip ?? false;
    if (hadWip !== wantWip(state.info, status)) return refreshAll();
    setState({ status });
  } catch {
    /* repo may be mid-operation */
  }
}

// Debounced reaction to file-system events from the watcher.
let fullTimer: number | undefined;
let statusTimer: number | undefined;
export function onRepoChanged(path: string, git: boolean) {
  if (path !== state.repo) return;
  if (git) {
    clearTimeout(fullTimer);
    clearTimeout(statusTimer);
    fullTimer = window.setTimeout(refreshAll, 150);
  } else {
    clearTimeout(statusTimer);
    statusTimer = window.setTimeout(refreshStatus, 400);
  }
}

export function openTerminal(open = true) {
  setState({ terminalOpen: open });
  updateUi({ terminalOpen: open });
}

export function currentBranch() {
  return state.info?.branches.find((b) => b.current) ?? null;
}

// ---------------------------------------------------------------------------------------------
// Per-repository graph view (branch filter / priorities)
// ---------------------------------------------------------------------------------------------

const EMPTY_VIEW: RepoView = { hidden: [], onlyLocal: false, hideTags: false, priorities: {} };
const samePath = (a: string, b: string) => a.replace(/\//g, "\\").toLowerCase() === b.replace(/\//g, "\\").toLowerCase();

export function repoView(): RepoView {
  const repo = state.repo;
  const repos = state.config?.repos ?? {};
  const key = repo ? Object.keys(repos).find((k) => samePath(k, repo)) : undefined;
  return { ...EMPTY_VIEW, ...(key ? repos[key] : {}) };
}

export function isFiltered(v: RepoView = repoView()) {
  return v.hidden.length > 0 || v.onlyLocal || v.hideTags;
}

export async function saveRepoView(view: RepoView) {
  const repo = state.repo;
  if (!repo) return;
  try {
    const config = await api.saveRepoView(repo, view);
    setState({ config });
    await refreshAll();
  } catch (e) {
    toastError(e);
  }
}

/** Shows or hides one ref (full name) in the graph. */
export function toggleRefHidden(fullRef: string) {
  const v = repoView();
  const hidden = v.hidden.includes(fullRef) ? v.hidden.filter((h) => h !== fullRef) : [...v.hidden, fullRef];
  return saveRepoView({ ...v, hidden });
}
