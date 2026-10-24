//! Built-in terminal: one pseudo-console session per repository (id = repo path).

use crate::config::ConfigState;
use crate::shell::{self, ShellKind};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    kind: ShellKind,
}

#[derive(Default)]
pub struct Terminals {
    sessions: Mutex<HashMap<String, Session>>,
}

#[derive(Serialize, Clone)]
struct TermData {
    id: String,
    data: String,
}

/// Splits `buf` into the longest valid UTF-8 prefix and an incomplete tail (kept for the next read).
fn take_utf8(buf: &mut Vec<u8>) -> String {
    match std::str::from_utf8(buf) {
        Ok(s) => {
            let s = s.to_string();
            buf.clear();
            s
        }
        Err(e) => {
            let valid = e.valid_up_to();
            if e.error_len().is_none() && buf.len() - valid < 4 {
                // Incomplete multi-byte sequence at the end.
                let s = String::from_utf8_lossy(&buf[..valid]).into_owned();
                buf.drain(..valid);
                s
            } else {
                let s = String::from_utf8_lossy(buf).into_owned();
                buf.clear();
                s
            }
        }
    }
}

fn open_session(
    app: &AppHandle,
    terms: &Terminals,
    shell_setting: &str,
    font_cols: (u16, u16),
    id: &str,
    cwd: &str,
) -> Result<(), String> {
    let mut sessions = terms.sessions.lock().unwrap();
    if let Some(s) = sessions.get_mut(id) {
        // Still alive?
        if matches!(s.child.try_wait(), Ok(None)) {
            return Ok(());
        }
        sessions.remove(id);
    }

    let spec = shell::resolve(shell_setting);
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize { rows: font_cols.1.max(5), cols: font_cols.0.max(20), pixel_width: 0, pixel_height: 0 })
        .map_err(|e| e.to_string())?;

    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("GIT_RUSTER", "1");
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    let app2 = app.clone();
    let id2 = id.to_string();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let data = take_utf8(&mut pending);
                    if !data.is_empty() {
                        let _ = app2.emit("term-data", TermData { id: id2.clone(), data });
                    }
                }
            }
        }
        let _ = app2.emit("term-exit", TermData { id: id2, data: String::new() });
    });

    sessions.insert(
        id.to_string(),
        Session { master: pair.master, writer, child, kind: spec.kind },
    );
    Ok(())
}

#[tauri::command]
pub fn term_open(
    app: AppHandle,
    terms: State<Terminals>,
    config: State<ConfigState>,
    id: String,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let shell = config.config.lock().unwrap().settings.shell.clone();
    open_session(&app, &terms, &shell, (cols, rows), &id, &cwd)
}

#[tauri::command]
pub fn term_write(terms: State<Terminals>, id: String, data: String) -> Result<(), String> {
    let mut sessions = terms.sessions.lock().unwrap();
    let s = sessions.get_mut(&id).ok_or("terminal is not running")?;
    s.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    s.writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn term_resize(terms: State<Terminals>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let sessions = terms.sessions.lock().unwrap();
    if let Some(s) = sessions.get(&id) {
        s.master
            .resize(PtySize { rows: rows.max(2), cols: cols.max(10), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn term_close(terms: State<Terminals>, id: String) {
    if let Some(mut s) = terms.sessions.lock().unwrap().remove(&id) {
        let _ = s.child.kill();
    }
}

/// Types a command line into the repo's terminal and presses Enter.
/// `commands` is a list of argv arrays that are chained with "&&" semantics.
#[tauri::command]
pub fn term_run(
    app: AppHandle,
    terms: State<Terminals>,
    config: State<ConfigState>,
    id: String,
    cwd: String,
    commands: Vec<Vec<String>>,
) -> Result<String, String> {
    let shell = config.config.lock().unwrap().settings.shell.clone();
    open_session(&app, &terms, &shell, (120, 30), &id, &cwd)?;
    let mut sessions = terms.sessions.lock().unwrap();
    let s = sessions.get_mut(&id).ok_or("terminal is not running")?;
    let line = shell::build_line(s.kind, &commands);
    s.writer
        .write_all(format!("{line}\r").as_bytes())
        .map_err(|e| e.to_string())?;
    s.writer.flush().map_err(|e| e.to_string())?;
    Ok(line)
}
