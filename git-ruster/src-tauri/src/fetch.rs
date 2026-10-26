//! Silent background `git fetch` (startup and periodic refresh). Unlike user-triggered
//! commands this does not go through the terminal: it only updates remote-tracking refs,
//! never prompts for credentials and is killed if it hangs.

use crate::git::git;
use crate::util::{blocking, hidden_command};
use serde::Serialize;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

const FETCH_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FetchResult {
    /// Remote-tracking refs or tags changed.
    pub changed: bool,
    /// Commits the local branches are behind their upstreams (sum over branches).
    pub behind: u32,
}

fn remote_refs(repo: &str) -> String {
    git(repo, &["for-each-ref", "--format=%(objectname) %(refname)", "refs/remotes", "refs/tags"]).unwrap_or_default()
}

fn behind_total(repo: &str) -> u32 {
    git(repo, &["for-each-ref", "--format=%(upstream:track)", "refs/heads"])
        .unwrap_or_default()
        .lines()
        .map(|l| crate::git::parse_track(l).1)
        .sum()
}

fn run_fetch(repo: &str) -> Result<(), String> {
    let mut child = hidden_command("git")
        .args(["-c", "credential.interactive=false", "fetch", "--all", "--prune", "--quiet"])
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run git: {e}"))?;
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + FETCH_TIMEOUT;
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("git fetch timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if status.success() {
        Ok(())
    } else {
        Err(reader.join().unwrap_or_default().trim().to_string())
    }
}

#[tauri::command]
pub async fn fetch_repo(repo: String) -> Result<FetchResult, String> {
    blocking(move || {
        let before = remote_refs(&repo);
        run_fetch(&repo)?;
        Ok(FetchResult { changed: remote_refs(&repo) != before, behind: behind_total(&repo) })
    })
    .await
}
