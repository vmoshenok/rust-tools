//! Shell selection and command-line building for the built-in terminal.

use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShellKind {
    Pwsh,
    PowerShell,
    Cmd,
    Bash,
}

pub struct ShellSpec {
    pub kind: ShellKind,
    pub program: String,
    pub args: Vec<String>,
}

fn in_path(exe: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(exe))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
}

fn git_bash() -> Option<String> {
    let candidates = [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ];
    candidates
        .iter()
        .find(|p| Path::new(p).is_file())
        .map(|s| s.to_string())
        .or_else(|| {
            // git.exe lives in <Git>\cmd\git.exe; bash in <Git>\bin\bash.exe
            let git = in_path("git.exe")?;
            let bash = Path::new(&git).parent()?.parent()?.join("bin").join("bash.exe");
            bash.is_file().then(|| bash.to_string_lossy().to_string())
        })
}

pub fn resolve(setting: &str) -> ShellSpec {
    let pwsh = || {
        in_path("pwsh.exe").or_else(|| {
            let p = r"C:\Program Files\PowerShell\7\pwsh.exe";
            Path::new(p).is_file().then(|| p.to_string())
        })
    };
    let spec = |kind, program: String| {
        let args = match kind {
            ShellKind::Pwsh | ShellKind::PowerShell => vec!["-NoLogo".to_string()],
            ShellKind::Bash => vec!["--login".to_string(), "-i".to_string()],
            ShellKind::Cmd => vec![],
        };
        ShellSpec { kind, program, args }
    };
    match setting {
        "pwsh" => {
            if let Some(p) = pwsh() {
                return spec(ShellKind::Pwsh, p);
            }
        }
        "powershell" => return spec(ShellKind::PowerShell, "powershell.exe".into()),
        "cmd" => return spec(ShellKind::Cmd, "cmd.exe".into()),
        "bash" => {
            if let Some(p) = git_bash() {
                return spec(ShellKind::Bash, p);
            }
        }
        _ => {}
    }
    match pwsh() {
        Some(p) => spec(ShellKind::Pwsh, p),
        None => spec(ShellKind::PowerShell, "powershell.exe".into()),
    }
}

fn is_plain(arg: &str) -> bool {
    !arg.is_empty()
        && arg != "--"
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '\\' | ':' | '=' | '+'))
        && !arg.starts_with('@')
}

pub fn quote(kind: ShellKind, arg: &str) -> String {
    if is_plain(arg) && !(kind == ShellKind::Bash && arg.contains('\\')) {
        return arg.to_string();
    }
    match kind {
        ShellKind::Pwsh | ShellKind::PowerShell => format!("'{}'", arg.replace('\'', "''")),
        ShellKind::Bash => format!("'{}'", arg.replace('\'', r"'\''")),
        ShellKind::Cmd => {
            if arg == "--" {
                "--".to_string()
            } else {
                format!("\"{}\"", arg.replace('"', "\\\""))
            }
        }
    }
}

/// Joins several commands so that each runs only if the previous one succeeded.
pub fn build_line(kind: ShellKind, commands: &[Vec<String>]) -> String {
    let parts: Vec<String> = commands
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| c.iter().map(|a| quote(kind, a)).collect::<Vec<_>>().join(" "))
        .collect();
    match kind {
        ShellKind::PowerShell => {
            // Windows PowerShell 5.1 has no `&&`.
            let mut line = String::new();
            let mut closers = 0;
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    line.push_str("; if ($?) { ");
                    closers += 1;
                }
                line.push_str(p);
            }
            line.push_str(&" }".repeat(closers));
            line
        }
        _ => parts.join(" && "),
    }
}
