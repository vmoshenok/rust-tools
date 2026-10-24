use std::path::{Path, PathBuf};
use std::process::Command;

/// Normalizes a path for display/storage: native separators, no trailing separator,
/// no `\\?\` prefix.
pub fn norm_path(p: &str) -> String {
    let mut s = p.trim().trim_matches('"').to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    #[cfg(windows)]
    {
        s = s.replace('/', "\\");
        // Upper-case drive letter so the same folder is not added twice.
        let b = s.as_bytes();
        if b.len() >= 2 && b[1] == b':' {
            s = s[..1].to_uppercase() + &s[1..];
        }
    }
    while s.len() > 3 && (s.ends_with('\\') || s.ends_with('/')) {
        s.pop();
    }
    s
}

pub fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        norm_path(a).eq_ignore_ascii_case(&norm_path(b))
    } else {
        norm_path(a) == norm_path(b)
    }
}

pub fn file_name(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

/// Walks up from `start` looking for a directory containing `.git` (dir or file).
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// `Command` that never flashes a console window on Windows.
pub fn hidden_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Runs a blocking closure on the blocking thread pool so commands never stall the UI thread.
pub async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}
