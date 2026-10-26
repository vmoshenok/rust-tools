// Typed wrappers around the Rust commands (src-tauri/src/*.rs).
import { invoke } from "@tauri-apps/api/core";

export interface Settings {
  shell: "auto" | "pwsh" | "powershell" | "cmd" | "bash";
  maxCommits: number;
  terminalFontSize: number;
}

export interface UiState {
  sidebarWidth?: number;
  detailsWidth?: number;
  terminalHeight?: number;
  terminalOpen?: boolean;
  collapsed?: Record<string, boolean>;
}

export interface RepoView {
  /** Full ref names hidden from the graph (refs/heads/x, refs/remotes/origin/x). */
  hidden: string[];
  onlyLocal: boolean;
  hideTags: boolean;
  /** Full ref name -> priority (1 = leftmost lane). */
  priorities: Record<string, number>;
}

export interface Config {
  folders: string[];
  selected: string | null;
  settings: Settings;
  repos: Record<string, RepoView>;
  ui: UiState | null;
}

export interface RepoEntry {
  path: string;
  name: string;
}

export interface FolderNode {
  path: string;
  name: string;
  kind: "repo" | "group" | "plain" | "missing";
  children: RepoEntry[];
}

export interface BranchInfo {
  name: string;
  sha: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  gone: boolean;
  current: boolean;
}

export interface RefTarget {
  name: string;
  sha: string;
}

export interface Stash {
  name: string;
  sha: string;
  message: string;
}

export type RepoState = "none" | "merge" | "rebase" | "cherry-pick" | "revert";

export interface RepoInfo {
  root: string;
  gitDir: string;
  branch: string | null;
  headSha: string | null;
  detached: boolean;
  state: RepoState;
  stateLabel: string;
  oursLabel: string;
  theirsLabel: string;
  branches: BranchInfo[];
  remoteBranches: RefTarget[];
  tags: RefTarget[];
  stashes: Stash[];
  remotes: string[];
}

export interface RefLabel {
  name: string;
  kind: "branch" | "remote" | "tag" | "head";
  current: boolean;
}

export interface GraphRow {
  sha: string;
  parents: string[];
  author: string;
  email: string;
  time: number;
  subject: string;
  refs: RefLabel[];
  col: number;
  color: number;
  /** [x1, y1, x2, y2, color]; y: 0 top, 1 middle, 2 bottom */
  segs: [number, number, number, number, number][];
  head: boolean;
  wip: boolean;
}

export interface GraphData {
  rows: GraphRow[];
  maxLanes: number;
}

export interface FileChange {
  path: string;
  origPath: string | null;
  x: string;
  y: string;
  staged: boolean;
  untracked: boolean;
  conflicted: boolean;
}

export interface CommitFile {
  path: string;
  oldPath: string | null;
  status: string;
}

export interface CommitDetails {
  sha: string;
  parents: string[];
  author: string;
  email: string;
  authorTime: number;
  committer: string;
  committerEmail: string;
  commitTime: number;
  message: string;
  files: CommitFile[];
}

export interface Blob {
  exists: boolean;
  binary: boolean;
  tooLarge: boolean;
  text: string;
}

export interface ConflictVersions {
  base: Blob;
  ours: Blob;
  theirs: Blob;
  work: Blob;
}

export interface FetchResult {
  /** remote-tracking refs or tags changed */
  changed: boolean;
  /** commits local branches are behind their upstreams (sum) */
  behind: number;
}

export const api = {
  getConfig: () => invoke<Config>("get_config"),
  addFolders: (paths: string[]) => invoke<Config>("add_folders", { paths }),
  removeFolder: (path: string) => invoke<Config>("remove_folder", { path }),
  setSelected: (path: string | null) => invoke<void>("set_selected", { path }),
  saveSettings: (settings: Settings) => invoke<Config>("save_settings", { settings }),
  saveUi: (ui: UiState) => invoke<void>("save_ui", { ui }),
  saveRepoView: (repo: string, view: RepoView) => invoke<Config>("save_repo_view", { repo, view }),
  listFolders: () => invoke<FolderNode[]>("list_folders"),

  repoInfo: (repo: string) => invoke<RepoInfo>("repo_info", { repo }),
  graph: (repo: string, limit: number, wip: boolean) => invoke<GraphData>("graph", { repo, limit, wip }),
  status: (repo: string) => invoke<FileChange[]>("status", { repo }),
  commitDetails: (repo: string, sha: string) => invoke<CommitDetails>("commit_details", { repo, sha }),
  compareFiles: (repo: string, from: string, to: string | null) => invoke<CommitFile[]>("compare_files", { repo, from, to }),
  readBlob: (repo: string, spec: string) => invoke<Blob>("read_blob", { repo, spec }),
  readWorkFile: (repo: string, path: string) => invoke<Blob>("read_work_file", { repo, path }),
  writeWorkFile: (repo: string, path: string, content: string) =>
    invoke<void>("write_work_file", { repo, path, content }),
  conflictVersions: (repo: string, path: string) => invoke<ConflictVersions>("conflict_versions", { repo, path }),
  readMergeMsg: (repo: string) => invoke<string | null>("read_merge_msg", { repo }),
  lastCommitMessage: (repo: string) => invoke<string>("last_commit_message", { repo }),
  writeTempFile: (name: string, content: string) => invoke<string>("write_temp_file", { name, content }),
  openInExplorer: (path: string) => invoke<void>("open_in_explorer", { path }),
  fetchRepo: (repo: string) => invoke<FetchResult>("fetch_repo", { repo }),

  termOpen: (id: string, cwd: string, cols: number, rows: number) =>
    invoke<void>("term_open", { id, cwd, cols, rows }),
  termWrite: (id: string, data: string) => invoke<void>("term_write", { id, data }),
  termResize: (id: string, cols: number, rows: number) => invoke<void>("term_resize", { id, cols, rows }),
  termClose: (id: string) => invoke<void>("term_close", { id }),
  termRun: (id: string, cwd: string, commands: string[][]) => invoke<string>("term_run", { id, cwd, commands }),

  watchRepo: (path: string | null) => invoke<void>("watch_repo", { path }),

  contextMenuStatus: () => invoke<boolean>("context_menu_status"),
  contextMenuInstall: () => invoke<void>("context_menu_install"),
  contextMenuUninstall: () => invoke<void>("context_menu_uninstall"),
};
