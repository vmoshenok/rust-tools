//! Watches the selected repository and tells the UI to refresh.
//! Emits `repo-changed` with `{ path, git }`: `git == true` when something inside `.git`
//! changed (refs, HEAD, index) and a full reload is needed; otherwise only the status.

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult, Debouncer};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
pub struct WatcherState {
    current: Mutex<Option<(String, Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>)>>,
}

#[derive(Serialize, Clone)]
struct Changed {
    path: String,
    git: bool,
}

fn is_noise(p: &Path) -> bool {
    let s = p.to_string_lossy().replace('\\', "/");
    s.contains("/.git/objects/") || s.ends_with(".lock") || s.contains("/.git/logs/")
}

#[tauri::command]
pub fn watch_repo(app: AppHandle, state: State<WatcherState>, path: Option<String>) -> Result<(), String> {
    let mut cur = state.current.lock().unwrap();
    if let (Some(p), Some((old, _))) = (&path, cur.as_ref()) {
        if p == old {
            return Ok(());
        }
    }
    *cur = None; // drops the old watcher
    let Some(path) = path else { return Ok(()) };
    if !Path::new(&path).is_dir() {
        return Ok(());
    }

    let repo = path.clone();
    let git_dir = Path::new(&path).join(".git");
    let mut debouncer = new_debouncer(Duration::from_millis(350), move |res: DebounceEventResult| {
        let Ok(events) = res else { return };
        let mut any = false;
        let mut git = false;
        for e in events.iter().filter(|e| !is_noise(&e.path)) {
            any = true;
            if e.path.starts_with(&git_dir) {
                git = true;
            }
        }
        if any {
            let _ = app.emit("repo-changed", Changed { path: repo.clone(), git });
        }
    })
    .map_err(|e| e.to_string())?;
    debouncer
        .watcher()
        .watch(Path::new(&path), RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    *cur = Some((path, debouncer));
    Ok(())
}
