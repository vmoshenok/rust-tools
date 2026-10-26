//! Read-only git queries (via the git CLI). Everything that changes a repository is
//! executed visibly in the built-in terminal instead (see `pty.rs`).

use crate::config::{ConfigState, RepoView};
use crate::graph::{self, GraphData, LogCommit, RefLabel};
use crate::util::{blocking, hidden_command, norm_path};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn git_raw(cwd: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = hidden_command("git")
        .args(["-c", "core.quotepath=false", "-c", "color.ui=false", "--no-pager"])
        .args(args)
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn git(cwd: &str, args: &[&str]) -> Result<String, String> {
    git_raw(cwd, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

// ---------------------------------------------------------------------------------------------
// Repo info
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub sha: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub gone: bool,
    pub current: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RefTarget {
    pub name: String,
    pub sha: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stash {
    pub name: String,
    pub sha: String,
    pub message: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub root: String,
    pub git_dir: String,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub detached: bool,
    /// "none" | "merge" | "rebase" | "cherry-pick" | "revert"
    pub state: String,
    pub state_label: String,
    pub ours_label: String,
    pub theirs_label: String,
    pub branches: Vec<BranchInfo>,
    pub remote_branches: Vec<RefTarget>,
    pub tags: Vec<RefTarget>,
    pub stashes: Vec<Stash>,
    pub remotes: Vec<String>,
}

fn read_trim(p: PathBuf) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

fn subject_of(repo: &str, rev: &str) -> String {
    git(repo, &["log", "-1", "--format=%s", rev])
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Extracts `x` from a merge message like `Merge branch 'x' into y`.
fn merge_source_from_msg(msg: &str) -> Option<String> {
    let first = msg.lines().next()?;
    let start = first.find('\'')?;
    let end = first[start + 1..].find('\'')?;
    Some(first[start + 1..start + 1 + end].to_string())
}

pub fn parse_track(track: &str) -> (u32, u32, bool) {
    let mut ahead = 0;
    let mut behind = 0;
    let gone = track.contains("gone");
    for part in track.trim_matches(|c| c == '[' || c == ']').split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.parse().unwrap_or(0);
        }
    }
    (ahead, behind, gone)
}

pub fn load_repo_info(repo: &str) -> Result<RepoInfo, String> {
    let top = git(repo, &["rev-parse", "--show-toplevel", "--absolute-git-dir"])?;
    let mut lines = top.lines();
    let root = norm_path(lines.next().unwrap_or(repo));
    let git_dir = norm_path(lines.next().unwrap_or(""));
    let mut info = RepoInfo {
        root: root.clone(),
        git_dir: git_dir.clone(),
        state: "none".into(),
        ..Default::default()
    };
    let r = root.as_str();

    info.branch = git(r, &["symbolic-ref", "-q", "--short", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    info.head_sha = git(r, &["rev-parse", "-q", "--verify", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    info.detached = info.branch.is_none() && info.head_sha.is_some();

    let refs = git(
        r,
        &[
            "for-each-ref",
            "--format=%(refname)%1f%(objectname)%1f%(*objectname)%1f%(upstream:short)%1f%(upstream:track)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )
    .unwrap_or_default();
    for line in refs.lines() {
        let f: Vec<&str> = line.split('\x1f').collect();
        if f.len() < 5 {
            continue;
        }
        let (refname, obj, peeled, upstream, track) = (f[0], f[1], f[2], f[3], f[4]);
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            let (ahead, behind, gone) = parse_track(track);
            info.branches.push(BranchInfo {
                name: name.to_string(),
                sha: obj.to_string(),
                upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
                ahead,
                behind,
                gone,
                current: info.branch.as_deref() == Some(name),
            });
        } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
            if name.ends_with("/HEAD") {
                continue;
            }
            info.remote_branches.push(RefTarget { name: name.to_string(), sha: obj.to_string() });
        } else if let Some(name) = refname.strip_prefix("refs/tags/") {
            let sha = if peeled.is_empty() { obj } else { peeled };
            info.tags.push(RefTarget { name: name.to_string(), sha: sha.to_string() });
        }
    }

    if let Ok(stashes) = git(r, &["stash", "list", "--format=%gd%x1f%H%x1f%s"]) {
        for line in stashes.lines() {
            let f: Vec<&str> = line.split('\x1f').collect();
            if f.len() >= 3 {
                info.stashes.push(Stash {
                    name: f[0].to_string(),
                    sha: f[1].to_string(),
                    message: f[2].to_string(),
                });
            }
        }
    }
    info.remotes = git(r, &["remote"])
        .unwrap_or_default()
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // In-progress operation detection.
    let gd = Path::new(&git_dir);
    let cur = info.branch.clone().unwrap_or_else(|| "HEAD".into());
    if gd.join("rebase-merge").is_dir() || gd.join("rebase-apply").is_dir() {
        let dir = if gd.join("rebase-merge").is_dir() { gd.join("rebase-merge") } else { gd.join("rebase-apply") };
        let head_name = read_trim(dir.join("head-name"))
            .map(|s| s.trim_start_matches("refs/heads/").to_string())
            .unwrap_or_else(|| "HEAD".into());
        let onto = read_trim(dir.join("onto")).unwrap_or_default();
        let step = read_trim(dir.join("msgnum")).or_else(|| read_trim(dir.join("next")));
        let total = read_trim(dir.join("end")).or_else(|| read_trim(dir.join("last")));
        let stopped = read_trim(dir.join("stopped-sha"));
        info.state = "rebase".into();
        info.state_label = format!(
            "Rebasing {} onto {}{}",
            head_name,
            short(&onto),
            match (step, total) {
                (Some(s), Some(t)) => format!(" (step {s}/{t})"),
                _ => String::new(),
            }
        );
        info.ours_label = format!("Upstream (onto {})", short(&onto));
        info.theirs_label = match stopped {
            Some(sha) => format!("Your commit {} ({})", short(&sha), subject_of(r, &sha)),
            None => format!("Your commit ({head_name})"),
        };
    } else if let Some(sha) = read_trim(gd.join("MERGE_HEAD")) {
        let msg = std::fs::read_to_string(gd.join("MERGE_MSG")).unwrap_or_default();
        let src = merge_source_from_msg(&msg).unwrap_or_else(|| short(&sha).to_string());
        info.state = "merge".into();
        info.state_label = format!("Merging {src} into {cur}");
        info.ours_label = format!("Yours ({cur})");
        info.theirs_label = format!("Theirs ({src})");
    } else if let Some(sha) = read_trim(gd.join("CHERRY_PICK_HEAD")) {
        info.state = "cherry-pick".into();
        info.state_label = format!("Cherry-picking {} ({})", short(&sha), subject_of(r, &sha));
        info.ours_label = format!("Yours ({cur})");
        info.theirs_label = format!("Cherry-picked {}", short(&sha));
    } else if let Some(sha) = read_trim(gd.join("REVERT_HEAD")) {
        info.state = "revert".into();
        info.state_label = format!("Reverting {} ({})", short(&sha), subject_of(r, &sha));
        info.ours_label = format!("Yours ({cur})");
        info.theirs_label = format!("Revert of {}", short(&sha));
    }
    Ok(info)
}

#[tauri::command]
pub async fn repo_info(repo: String) -> Result<RepoInfo, String> {
    blocking(move || load_repo_info(&repo)).await
}

// ---------------------------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------------------------

/// Which refs the graph shows, according to the repository's view settings.
struct VisibleRefs {
    locals: Vec<usize>,
    remotes: Vec<usize>,
    tags: bool,
}

fn visible_refs(info: &RepoInfo, view: &RepoView) -> VisibleRefs {
    let hidden = |full: String| view.hidden.contains(&full);
    let locals: Vec<usize> = (0..info.branches.len())
        .filter(|&i| !hidden(format!("refs/heads/{}", info.branches[i].name)))
        .collect();
    let remotes = (0..info.remote_branches.len())
        .filter(|&i| {
            let name = &info.remote_branches[i].name;
            if hidden(format!("refs/remotes/{name}")) {
                return false;
            }
            !view.only_local
                || locals
                    .iter()
                    .any(|&l| info.branches[l].upstream.as_deref() == Some(name.as_str()))
        })
        .collect();
    VisibleRefs { locals, remotes, tags: !view.hide_tags }
}

#[tauri::command]
pub async fn graph(
    config: tauri::State<'_, ConfigState>,
    repo: String,
    limit: usize,
    wip: bool,
) -> Result<GraphData, String> {
    let view = config.config.lock().unwrap().repo_view(&repo);
    blocking(move || {
        let info = load_repo_info(&repo)?;
        let r = info.root.as_str();
        let vis = visible_refs(&info, &view);
        let limit_arg = format!("-n{}", limit.max(50));
        let mut args: Vec<String> = vec![
            "log".into(),
            "--date-order".into(),
            limit_arg,
            "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%s%x1e".into(),
        ];
        if view.is_filtered() {
            args.extend(vis.locals.iter().map(|&i| format!("refs/heads/{}", info.branches[i].name)));
            args.extend(vis.remotes.iter().map(|&i| format!("refs/remotes/{}", info.remote_branches[i].name)));
            if vis.tags {
                args.push("--tags".into());
            }
        } else {
            args.extend(["--branches".into(), "--remotes".into(), "--tags".into()]);
        }
        if info.head_sha.is_some() {
            args.push("HEAD".into());
        }
        args.push("--".into());
        let has_revs = info.head_sha.is_some() || !vis.locals.is_empty() || !vis.remotes.is_empty() || (vis.tags && !info.tags.is_empty());
        let out = if has_revs {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            git(r, &a)?
        } else {
            String::new() // empty repository / nothing selected
        };

        // sha -> ref labels (only for refs that are visible)
        let mut labels: HashMap<String, Vec<RefLabel>> = HashMap::new();
        if info.detached {
            if let Some(h) = &info.head_sha {
                labels.entry(h.clone()).or_default().push(RefLabel {
                    name: "HEAD".into(),
                    kind: "head".into(),
                    current: true,
                });
            }
        }
        for &i in &vis.locals {
            let b = &info.branches[i];
            labels.entry(b.sha.clone()).or_default().push(RefLabel {
                name: b.name.clone(),
                kind: "branch".into(),
                current: b.current,
            });
        }
        // The current branch is always labelled, even when hidden.
        if let Some(b) = info.branches.iter().find(|b| b.current) {
            let l = labels.entry(b.sha.clone()).or_default();
            if !l.iter().any(|x| x.kind == "branch" && x.name == b.name) {
                l.push(RefLabel { name: b.name.clone(), kind: "branch".into(), current: true });
            }
        }
        for &i in &vis.remotes {
            let b = &info.remote_branches[i];
            labels.entry(b.sha.clone()).or_default().push(RefLabel {
                name: b.name.clone(),
                kind: "remote".into(),
                current: false,
            });
        }
        if vis.tags {
            for t in &info.tags {
                labels.entry(t.sha.clone()).or_default().push(RefLabel {
                    name: t.name.clone(),
                    kind: "tag".into(),
                    current: false,
                });
            }
        }

        // Priority lanes: tips ordered by priority (1 = leftmost).
        let mut prio: Vec<(u32, String)> = view
            .priorities
            .iter()
            .filter(|(_, &p)| p > 0)
            .filter_map(|(full, &p)| {
                if let Some(n) = full.strip_prefix("refs/heads/") {
                    info.branches.iter().find(|b| b.name == n).map(|b| (p, b.sha.clone()))
                } else if let Some(n) = full.strip_prefix("refs/remotes/") {
                    info.remote_branches.iter().find(|b| b.name == n).map(|b| (p, b.sha.clone()))
                } else {
                    None
                }
            })
            .collect();
        prio.sort_by_key(|(p, _)| *p);
        let prio_tips: Vec<String> = prio.into_iter().map(|(_, sha)| sha).collect();

        let mut commits = Vec::new();
        if wip {
            commits.push(LogCommit {
                sha: "WIP".into(),
                parents: info.head_sha.iter().cloned().collect(),
                author: String::new(),
                email: String::new(),
                time: 0,
                subject: String::new(),
            });
        }
        for rec in out.split('\x1e') {
            let rec = rec.trim_start_matches(['\n', '\r']);
            if rec.is_empty() {
                continue;
            }
            let f: Vec<&str> = rec.splitn(6, '\x1f').collect();
            if f.len() < 6 {
                continue;
            }
            commits.push(LogCommit {
                sha: f[0].to_string(),
                parents: f[1].split_whitespace().map(String::from).collect(),
                author: f[2].to_string(),
                email: f[3].to_string(),
                time: f[4].parse().unwrap_or(0),
                subject: f[5].to_string(),
            });
        }
        Ok(graph::layout(commits, &mut labels, info.head_sha.as_deref(), &prio_tips))
    })
    .await
}

// ---------------------------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub orig_path: Option<String>,
    /// Index status letter (porcelain v1 X)
    pub x: String,
    /// Work-tree status letter (porcelain v1 Y)
    pub y: String,
    pub staged: bool,
    pub untracked: bool,
    pub conflicted: bool,
}

#[tauri::command]
pub async fn status(repo: String) -> Result<Vec<FileChange>, String> {
    blocking(move || {
        let out = git_raw(&repo, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        let text = String::from_utf8_lossy(&out);
        let mut parts = text.split('\0').filter(|s| !s.is_empty());
        let mut res = Vec::new();
        while let Some(entry) = parts.next() {
            if entry.len() < 4 {
                continue;
            }
            let x = &entry[0..1];
            let y = &entry[1..2];
            let path = entry[3..].to_string();
            let orig_path = if x == "R" || x == "C" { parts.next().map(String::from) } else { None };
            let xy = &entry[0..2];
            let conflicted = matches!(xy, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU");
            res.push(FileChange {
                path,
                orig_path,
                x: x.to_string(),
                y: y.to_string(),
                staged: !conflicted && x != " " && x != "?",
                untracked: x == "?",
                conflicted,
            });
        }
        res.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
        Ok(res)
    })
    .await
}

// ---------------------------------------------------------------------------------------------
// Commit details
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetails {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub author_time: i64,
    pub committer: String,
    pub committer_email: String,
    pub commit_time: i64,
    pub message: String,
    pub files: Vec<CommitFile>,
}

fn parse_name_status(raw: &[u8]) -> Vec<CommitFile> {
    let text = String::from_utf8_lossy(raw);
    let mut parts = text.split('\0').filter(|s| !s.is_empty());
    let mut files = Vec::new();
    while let Some(status) = parts.next() {
        let letter = status.chars().next().unwrap_or('M').to_string();
        if letter == "R" || letter == "C" {
            let old = parts.next().map(String::from);
            if let Some(new) = parts.next() {
                files.push(CommitFile { path: new.to_string(), old_path: old, status: letter });
            }
        } else if let Some(p) = parts.next() {
            files.push(CommitFile { path: p.to_string(), old_path: None, status: letter });
        }
    }
    files
}

#[tauri::command]
pub async fn commit_details(repo: String, sha: String) -> Result<CommitDetails, String> {
    blocking(move || {
        let out = git(
            &repo,
            &["show", "-s", "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%cn%x1f%ce%x1f%ct%x1f%B", &sha],
        )?;
        let f: Vec<&str> = out.splitn(9, '\x1f').collect();
        if f.len() < 9 {
            return Err("unexpected git show output".into());
        }
        let parents: Vec<String> = f[1].split_whitespace().map(String::from).collect();
        let raw = match parents.first() {
            Some(p) => git_raw(&repo, &["diff-tree", "-r", "-z", "-M", "--name-status", p, &sha])?,
            None => git_raw(&repo, &["diff-tree", "--root", "-r", "-z", "-M", "--name-status", &sha])?,
        };
        Ok(CommitDetails {
            sha: f[0].to_string(),
            parents,
            author: f[2].to_string(),
            email: f[3].to_string(),
            author_time: f[4].parse().unwrap_or(0),
            committer: f[5].to_string(),
            committer_email: f[6].to_string(),
            commit_time: f[7].parse().unwrap_or(0),
            message: f[8].trim_end().to_string(),
            files: parse_name_status(&raw),
        })
    })
    .await
}

/// Files that differ between `from` and `to` (a revision), or between `from` and the working
/// tree when `to` is None (untracked files are reported as added).
#[tauri::command]
pub async fn compare_files(repo: String, from: String, to: Option<String>) -> Result<Vec<CommitFile>, String> {
    blocking(move || {
        let mut args = vec!["diff", "--name-status", "-z", "-M", from.as_str()];
        if let Some(t) = &to {
            args.push(t.as_str());
        }
        args.push("--");
        let mut files = parse_name_status(&git_raw(&repo, &args)?);
        if to.is_none() {
            let raw = git_raw(&repo, &["ls-files", "--others", "--exclude-standard", "-z"])?;
            for p in String::from_utf8_lossy(&raw).split('\0').filter(|s| !s.is_empty()) {
                files.push(CommitFile { path: p.to_string(), old_path: None, status: "A".into() });
            }
        }
        files.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
        Ok(files)
    })
    .await
}

// ---------------------------------------------------------------------------------------------
// File contents
// ---------------------------------------------------------------------------------------------

const MAX_TEXT: usize = 8 * 1024 * 1024;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Blob {
    pub exists: bool,
    pub binary: bool,
    pub too_large: bool,
    pub text: String,
}

fn blob_from_bytes(bytes: Vec<u8>) -> Blob {
    if bytes.len() > MAX_TEXT {
        return Blob { exists: true, too_large: true, ..Default::default() };
    }
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return Blob { exists: true, binary: true, ..Default::default() };
    }
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };
    Blob { exists: true, text: text.trim_start_matches('\u{feff}').to_string(), ..Default::default() }
}

fn read_spec(repo: &str, spec: &str) -> Blob {
    match git_raw(repo, &["cat-file", "blob", spec]) {
        Ok(b) => blob_from_bytes(b),
        Err(_) => Blob::default(),
    }
}

/// Reads a blob by git object spec, e.g. `HEAD:src/main.rs`, `:src/main.rs` (index), `:2:file`.
#[tauri::command]
pub async fn read_blob(repo: String, spec: String) -> Result<Blob, String> {
    blocking(move || Ok(read_spec(&repo, &spec))).await
}

fn read_work(repo: &str, path: &str) -> Blob {
    match std::fs::read(Path::new(repo).join(path)) {
        Ok(b) => blob_from_bytes(b),
        Err(_) => Blob::default(),
    }
}

#[tauri::command]
pub async fn read_work_file(repo: String, path: String) -> Result<Blob, String> {
    blocking(move || Ok(read_work(&repo, &path))).await
}

#[tauri::command]
pub async fn write_work_file(repo: String, path: String, content: String) -> Result<(), String> {
    blocking(move || std::fs::write(Path::new(&repo).join(&path), content).map_err(|e| e.to_string())).await
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConflictVersions {
    pub base: Blob,
    pub ours: Blob,
    pub theirs: Blob,
    pub work: Blob,
}

#[tauri::command]
pub async fn conflict_versions(repo: String, path: String) -> Result<ConflictVersions, String> {
    blocking(move || {
        Ok(ConflictVersions {
            base: read_spec(&repo, &format!(":1:{path}")),
            ours: read_spec(&repo, &format!(":2:{path}")),
            theirs: read_spec(&repo, &format!(":3:{path}")),
            work: read_work(&repo, &path),
        })
    })
    .await
}

fn strip_comments(s: &str) -> String {
    s.lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Message prepared by git for an in-progress merge / cherry-pick / revert / rebase stop.
#[tauri::command]
pub async fn read_merge_msg(repo: String) -> Result<Option<String>, String> {
    blocking(move || {
        let gd = git(&repo, &["rev-parse", "--absolute-git-dir"])?;
        let gd = PathBuf::from(gd.trim());
        for f in ["MERGE_MSG", "rebase-merge/message"] {
            if let Ok(s) = std::fs::read_to_string(gd.join(f)) {
                return Ok(Some(strip_comments(&s)));
            }
        }
        Ok(None)
    })
    .await
}

#[tauri::command]
pub async fn last_commit_message(repo: String) -> Result<String, String> {
    blocking(move || git(&repo, &["log", "-1", "--format=%B"]).map(|s| s.trim_end().to_string())).await
}

/// Writes a temp file (commit message, pathspec list) and returns its absolute path.
#[tauri::command]
pub fn write_temp_file(name: String, content: String) -> Result<String, String> {
    let dir = std::env::temp_dir().join("git-ruster");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = dir.join(safe);
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(norm_path(&path.to_string_lossy()))
}

#[tauri::command]
pub fn open_in_explorer(path: String) -> Result<(), String> {
    let p = norm_path(&path);
    let mut cmd = std::process::Command::new("explorer.exe");
    if Path::new(&p).is_file() {
        cmd.arg(format!("/select,{p}"));
    } else {
        cmd.arg(&p);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}
