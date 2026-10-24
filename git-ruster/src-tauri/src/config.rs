use crate::util::{file_name, find_repo_root, norm_path, same_path};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// "auto" | "pwsh" | "powershell" | "cmd" | "bash"
    pub shell: String,
    pub max_commits: usize,
    pub terminal_font_size: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shell: "auto".into(),
            max_commits: 3000,
            terminal_font_size: 13,
        }
    }
}

/// Per-repository graph filter / ordering.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct RepoView {
    /// Full ref names (`refs/heads/x`, `refs/remotes/origin/x`) not shown in the graph.
    pub hidden: Vec<String>,
    /// Show only local branches and their upstream (remote-tracking) branches.
    pub only_local: bool,
    pub hide_tags: bool,
    /// Full ref name -> priority (1 = leftmost lane).
    pub priorities: HashMap<String, u32>,
}

impl RepoView {
    pub fn is_filtered(&self) -> bool {
        !self.hidden.is_empty() || self.only_local || self.hide_tags
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub folders: Vec<String>,
    pub selected: Option<String>,
    pub settings: Settings,
    /// Repo path -> graph view settings.
    pub repos: HashMap<String, RepoView>,
    /// Free-form UI state owned by the frontend (panel sizes etc.).
    pub ui: serde_json::Value,
}

impl Config {
    pub fn repo_view(&self, repo: &str) -> RepoView {
        self.repos
            .iter()
            .find(|(k, _)| same_path(k, repo))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
}

pub struct ConfigState {
    pub config: Mutex<Config>,
    path: PathBuf,
}

impl ConfigState {
    pub fn load(app: &AppHandle) -> Self {
        let dir = app
            .path()
            .app_config_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("git-ruster"));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        let config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            config: Mutex::new(config),
            path,
        }
    }

    pub fn save(&self, cfg: &Config) {
        if let Ok(s) = serde_json::to_string_pretty(cfg) {
            let _ = std::fs::write(&self.path, s);
        }
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut Config) -> R) -> (R, Config) {
        let mut cfg = self.config.lock().unwrap();
        let r = f(&mut cfg);
        self.save(&cfg);
        (r, cfg.clone())
    }
}

/// First command-line argument (after the exe) that is an existing directory.
pub fn path_from_args(args: &[String]) -> Option<String> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(|a| norm_path(a))
        .find(|a| Path::new(a).is_dir())
}

/// Resolves the folder to add: the enclosing repository root if there is one.
fn resolve_folder(path: &str) -> String {
    let p = PathBuf::from(norm_path(path));
    match find_repo_root(&p) {
        Some(root) => norm_path(&root.to_string_lossy()),
        None => norm_path(path),
    }
}

/// Is `path` already reachable from the folder list (directly or as a repo inside a group)?
fn is_listed(cfg: &Config, path: &str) -> bool {
    cfg.folders.iter().any(|f| {
        if same_path(f, path) {
            return true;
        }
        if Path::new(f).join(".git").exists() {
            return false;
        }
        scan_repos(Path::new(f)).iter().any(|r| same_path(&r.path, path))
    })
}

fn add_paths(cfg: &mut Config, paths: &[String]) -> Option<String> {
    let mut last = None;
    for p in paths {
        let folder = resolve_folder(p);
        if !is_listed(cfg, &folder) {
            cfg.folders.push(folder.clone());
        }
        last = Some(folder);
    }
    last
}

/// Called for `git-ruster.exe <folder>` (Explorer context menu): adds and selects the folder.
pub fn open_path(app: &AppHandle, path: &str) {
    let state = app.state::<ConfigState>();
    let (_, cfg) = state.update(|cfg| {
        if let Some(sel) = add_paths(cfg, &[path.to_string()]) {
            cfg.selected = Some(sel);
        }
    });
    let _ = app.emit("config-changed", cfg);
}

#[tauri::command]
pub fn get_config(state: State<ConfigState>) -> Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn add_folders(state: State<ConfigState>, paths: Vec<String>) -> Config {
    state
        .update(|cfg| {
            if let Some(sel) = add_paths(cfg, &paths) {
                cfg.selected = Some(sel);
            }
        })
        .1
}

#[tauri::command]
pub fn remove_folder(state: State<ConfigState>, path: String) -> Config {
    state
        .update(|cfg| {
            cfg.folders.retain(|f| !same_path(f, &path));
            if cfg.selected.as_deref().is_some_and(|s| same_path(s, &path)) {
                cfg.selected = None;
            }
        })
        .1
}

#[tauri::command]
pub fn set_selected(state: State<ConfigState>, path: Option<String>) {
    state.update(|cfg| cfg.selected = path);
}

#[tauri::command]
pub fn save_settings(state: State<ConfigState>, settings: Settings) -> Config {
    state.update(|cfg| cfg.settings = settings).1
}

#[tauri::command]
pub fn save_repo_view(state: State<ConfigState>, repo: String, view: RepoView) -> Config {
    state
        .update(|cfg| {
            cfg.repos.retain(|k, _| !same_path(k, &repo));
            if view != RepoView::default() {
                cfg.repos.insert(norm_path(&repo), view);
            }
        })
        .1
}

#[tauri::command]
pub fn save_ui(state: State<ConfigState>, ui: serde_json::Value) {
    state.update(|cfg| cfg.ui = ui);
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepoEntry {
    pub path: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FolderNode {
    pub path: String,
    pub name: String,
    /// "repo" | "group" (folder containing repos) | "plain" (no repos) | "missing"
    pub kind: String,
    pub children: Vec<RepoEntry>,
}

const SKIP_DIRS: &[&str] = &["node_modules", "target", "dist", "build", "bin", "obj", "vendor"];

/// Finds git repositories up to 3 levels below `root` (not descending into repos).
fn scan_repos(root: &Path) -> Vec<RepoEntry> {
    fn walk(dir: &Path, root: &Path, depth: u32, out: &mut Vec<RepoEntry>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut dirs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        dirs.sort_by_key(|p| p.to_string_lossy().to_lowercase());
        for d in dirs {
            let name = file_name(&d.to_string_lossy());
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if d.join(".git").exists() {
                let rel = d
                    .strip_prefix(root)
                    .map(|r| r.to_string_lossy().to_string())
                    .unwrap_or(name);
                out.push(RepoEntry {
                    path: norm_path(&d.to_string_lossy()),
                    name: rel,
                });
            } else if depth < 3 {
                walk(&d, root, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, 1, &mut out);
    out
}

#[tauri::command]
pub async fn list_folders(state: State<'_, ConfigState>) -> Result<Vec<FolderNode>, String> {
    let folders = state.config.lock().unwrap().folders.clone();
    crate::util::blocking(move || {
        Ok(folders
            .iter()
            .map(|f| {
                let p = Path::new(f);
                let name = file_name(f);
                if !p.is_dir() {
                    return FolderNode { path: f.clone(), name, kind: "missing".into(), children: vec![] };
                }
                if p.join(".git").exists() {
                    return FolderNode { path: f.clone(), name, kind: "repo".into(), children: vec![] };
                }
                let children = scan_repos(p);
                let kind = if children.is_empty() { "plain" } else { "group" };
                FolderNode { path: f.clone(), name, kind: kind.into(), children }
            })
            .collect())
    })
    .await
}
