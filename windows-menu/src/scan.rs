//! Collects every place an Explorer context-menu item can come from.
//!
//! * Classic static verbs:   `<class>\shell\<verb>` (+ `\command`), under
//!   `HKCU\Software\Classes` and `HKLM\Software\Classes` (merged as HKCR).
//! * Classic COM handlers:   `<class>\shellex\ContextMenuHandlers\<name>` -> CLSID -> DLL.
//! * Windows 11 menu items:  `windows.fileExplorerContextMenus` in packaged apps'
//!   `AppxManifest.xml` (IExplorerCommand COM classes).
//!
//! Classic items are what Windows 11 shows under "Show more options".

use crate::reg::{wide, Hive, Key};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use windows_sys::Win32::System::Registry::{REG_SZ, REG_VALUE_TYPE};
use windows_sys::Win32::UI::Shell::SHLoadIndirectString;

pub const BLOCKED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked";
const COMMAND_STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\CommandStore\shell";
const PACKAGES: &str = r"Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";

/// Classes whose menus apply to many objects, with a human-readable label.
const SCOPES: &[(&str, &str)] = &[
    ("*", "All files"),
    ("AllFilesystemObjects", "Files and folders"),
    ("Directory", "Folders"),
    ("Directory\\Background", "Folder background"),
    ("Folder", "Folders (incl. virtual)"),
    ("Drive", "Drives"),
    ("DesktopBackground", "Desktop background"),
    ("LibraryFolder\\background", "Library background"),
    ("Unknown", "Unknown file types"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Verb,
    Handler,
    Packaged,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Verb => "Classic verb",
            Kind::Handler => "Classic shell extension",
            Kind::Packaged => "Windows 11 (packaged app)",
        }
    }
}

#[derive(Clone)]
pub struct Entry {
    /// Caption as shown in the table (submenu items are prefixed with their parents).
    pub caption: String,
    /// The item's own caption, as stored in the registry.
    pub title: String,
    pub scope: String,
    pub kind: Kind,
    pub status: String,
    pub hive: Hive,
    /// Key path under `hive` (registry entries), or the manifest file (packaged).
    pub key: String,
    pub command: String,
    /// Value name holding the caption (`MUIVerb` or `""`); `None` = not editable.
    pub caption_value: Option<String>,
    /// Command line editable at `<key>\command` (default value) with this type.
    pub command_type: Option<REG_VALUE_TYPE>,
    pub clsid: Option<String>,
    pub disabled: bool,
}

impl Entry {
    pub fn location(&self) -> String {
        match self.kind {
            Kind::Packaged => self.key.clone(),
            _ => format!("{}\\{}", self.hive.short(), self.key),
        }
    }

    pub fn column(&self, col: usize) -> String {
        match col {
            0 => self.caption.clone(),
            1 => self.kind.label().to_string(),
            2 => self.scope.clone(),
            3 => self.status.clone(),
            4 => self.location(),
            _ => self.command.clone(),
        }
    }
}

pub const COLUMNS: &[(&str, i32)] =
    &[("Caption", 220), ("Type", 150), ("Shown on", 150), ("Status", 110), ("Registry path / source", 380), ("Command", 420)];

/// Resolves `@dll,-id` / `@{package?ms-resource:...}` strings.
pub fn resolve_indirect(s: &str) -> String {
    if !s.starts_with('@') {
        return s.to_string();
    }
    let src = wide(s);
    let mut buf = [0u16; 1024];
    let hr = unsafe { SHLoadIndirectString(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, std::ptr::null()) };
    if hr >= 0 {
        crate::reg::from_wide(&buf)
    } else {
        s.to_string()
    }
}

/// Strips `&` accelerator markers (`&&` stays a literal `&`).
fn strip_accel(s: &str) -> String {
    s.replace("&&", "\u{1}").replace('&', "").replace('\u{1}', "&")
}

fn classes(hive: Hive) -> &'static str {
    match hive {
        Hive::ClassesRoot => "",
        _ => "Software\\Classes\\",
    }
}

struct Scanner {
    out: Vec<Entry>,
    blocked: BTreeSet<String>,
    clsid_cache: HashMap<String, (String, String)>,
}

impl Scanner {
    fn is_blocked(&self, clsid: &str) -> bool {
        self.blocked.contains(&clsid.to_ascii_uppercase())
    }

    /// (class name, DLL path) of a COM class, looked up like HKCR does (HKCU first).
    fn clsid_info(&mut self, clsid: &str) -> (String, String) {
        let id = clsid.to_ascii_uppercase();
        if let Some(v) = self.clsid_cache.get(&id) {
            return v.clone();
        }
        let mut name = String::new();
        let mut dll = String::new();
        for hive in [Hive::CurrentUser, Hive::LocalMachine] {
            if let Some(k) = Key::open(hive, &format!("Software\\Classes\\CLSID\\{id}")) {
                if name.is_empty() {
                    name = k.get_str("").unwrap_or_default();
                }
                if dll.is_empty() {
                    dll = k.sub("InprocServer32").and_then(|s| s.get_str("")).unwrap_or_default();
                }
                if dll.is_empty() {
                    dll = k.sub("LocalServer32").and_then(|s| s.get_str("")).unwrap_or_default();
                }
            }
        }
        let v = (resolve_indirect(&name), dll);
        self.clsid_cache.insert(id, v.clone());
        v
    }

    fn com_description(&mut self, clsid: &str, how: &str) -> String {
        let (name, dll) = self.clsid_info(clsid);
        let mut s = format!("[{how}] {clsid}");
        if !name.is_empty() {
            s += &format!("  {name}");
        }
        if !dll.is_empty() {
            s += &format!("  ->  {dll}");
        }
        s
    }

    /// `<class>\shell` and `<class>\shellex` of one class in one hive.
    fn scan_class(&mut self, hive: Hive, class: &str, scope: &str) {
        let base = format!("{}{class}", classes(hive));
        self.scan_shell(hive, &format!("{base}\\shell"), scope, "", 0);
        self.scan_handlers(hive, &format!("{base}\\shellex\\ContextMenuHandlers"), scope);
    }

    fn scan_shell(&mut self, hive: Hive, shell: &str, scope: &str, prefix: &str, depth: u32) {
        let Some(k) = Key::open(hive, shell) else { return };
        for verb in k.subkeys() {
            self.scan_verb(hive, &format!("{shell}\\{verb}"), &verb, scope, prefix, depth);
        }
    }

    fn scan_verb(&mut self, hive: Hive, path: &str, verb: &str, scope: &str, prefix: &str, depth: u32) {
        let Some(k) = Key::open(hive, path) else { return };

        let (caption_value, raw) = match k.get_str("MUIVerb") {
            Some(s) if !s.is_empty() => ("MUIVerb", s),
            _ => ("", k.get_str("").unwrap_or_default()),
        };
        let mut caption = strip_accel(&resolve_indirect(&raw));
        if caption.is_empty() {
            caption = verb.to_string();
        }
        let title = caption;
        let caption = format!("{prefix}{title}");

        let cmd_key = k.sub("command");
        let cmd = cmd_key.as_ref().and_then(|c| c.get_str_typed(""));
        let mut command_type = Some(REG_SZ);
        let mut command = String::new();
        if let Some((s, ty)) = cmd.filter(|(s, _)| !s.is_empty()) {
            command = s;
            command_type = Some(ty);
        } else if let Some(clsid) = cmd_key.as_ref().and_then(|c| c.get_str("DelegateExecute")) {
            command = self.com_description(&clsid, "DelegateExecute");
            command_type = None;
        } else if let Some(clsid) = k.get_str("ExplorerCommandHandler") {
            command = self.com_description(&clsid, "ExplorerCommandHandler");
            command_type = None;
        }

        let sub_commands = k.get_str("SubCommands");
        let ext_sub = k.get_str("ExtendedSubCommandsKey");
        let is_parent = sub_commands.is_some() || ext_sub.is_some() || k.sub("shell").is_some();
        if command.is_empty() && is_parent {
            command = "(submenu)".to_string();
            command_type = None;
        }

        let mut status = Vec::new();
        let disabled = k.has_value("LegacyDisable");
        if disabled {
            status.push("Disabled");
        }
        if k.has_value("ProgrammaticAccessOnly") {
            status.push("Hidden (programmatic)");
        }
        if k.has_value("Extended") {
            status.push("Shift+right-click only");
        }
        if k.has_value("AppliesTo") {
            status.push("Conditional");
        }

        self.out.push(Entry {
            caption: caption.clone(),
            scope: scope.to_string(),
            title,
            kind: Kind::Verb,
            status: status.join(", "),
            hive,
            key: path.to_string(),
            command,
            caption_value: Some(caption_value.to_string()),
            command_type,
            clsid: None,
            disabled,
        });

        if depth >= 4 {
            return;
        }
        let child_prefix = format!("{caption} > ");
        if let Some(list) = sub_commands.filter(|s| !s.trim().is_empty()) {
            for name in list.split(';').map(str::trim).filter(|s| !s.is_empty()) {
                let p = format!("{COMMAND_STORE}\\{name}");
                self.scan_verb(Hive::LocalMachine, &p, name, scope, &child_prefix, depth + 1);
            }
        } else if let Some(ext) = ext_sub.filter(|s| !s.trim().is_empty()) {
            // Relative to HKCR: look at both halves of the merged view.
            for h in [Hive::CurrentUser, Hive::LocalMachine] {
                let p = format!("{}{}\\shell", classes(h), ext.trim_end_matches('\\'));
                self.scan_shell(h, &p, scope, &child_prefix, depth + 1);
            }
        } else {
            self.scan_shell(hive, &format!("{path}\\shell"), scope, &child_prefix, depth + 1);
        }
    }

    fn scan_handlers(&mut self, hive: Hive, path: &str, scope: &str) {
        let Some(k) = Key::open(hive, path) else { return };
        for name in k.subkeys() {
            let Some(h) = k.sub(&name) else { continue };
            let value = h.get_str("").unwrap_or_default();
            let clsid = if value.trim().starts_with('{') {
                value.trim().to_string()
            } else if name.starts_with('{') {
                name.clone()
            } else {
                continue;
            };
            let (class_name, _) = self.clsid_info(&clsid);
            let caption = if class_name.is_empty() || class_name.eq_ignore_ascii_case(&name) {
                name.clone()
            } else {
                format!("{name}  ({class_name})")
            };
            let blocked = self.is_blocked(&clsid);
            let command = self.com_description(&clsid, "COM handler");
            self.out.push(Entry {
                title: caption.clone(),
                caption,
                scope: scope.to_string(),
                kind: Kind::Handler,
                status: if blocked { "Blocked".into() } else { String::new() },
                hive,
                key: format!("{path}\\{name}"),
                command,
                caption_value: None,
                command_type: None,
                clsid: Some(clsid),
                disabled: blocked,
            });
        }
    }

    /// Per-file-type menus: every `.ext` -> ProgID, plus `SystemFileAssociations`.
    fn scan_file_types(&mut self) {
        let mut progids: BTreeMap<String, String> = BTreeMap::new();
        let mut exts = BTreeSet::new();
        for hive in [Hive::CurrentUser, Hive::LocalMachine] {
            let Some(k) = Key::open(hive, "Software\\Classes") else { continue };
            for name in k.subkeys().into_iter().filter(|n| n.starts_with('.')) {
                if let Some(p) = k.sub(&name).and_then(|e| e.get_str("")).filter(|p| !p.is_empty()) {
                    progids.entry(p).or_insert_with(|| name.clone());
                }
                exts.insert(name);
            }
        }
        for hive in [Hive::CurrentUser, Hive::LocalMachine] {
            for (progid, ext) in &progids {
                self.scan_class(hive, progid, &format!("{ext} files ({progid})"));
            }
            for ext in &exts {
                self.scan_class(hive, ext, &format!("{ext} files"));
            }
            let sfa = "SystemFileAssociations";
            if let Some(k) = Key::open(hive, &format!("{}{sfa}", classes(hive))) {
                for name in k.subkeys() {
                    self.scan_class(hive, &format!("{sfa}\\{name}"), &format!("{name} files (system)"));
                }
            }
        }
    }

    /// Windows 11 top-level items declared by packaged / sparse-packaged apps.
    fn scan_packages(&mut self) {
        let Some(k) = Key::open(Hive::ClassesRoot, PACKAGES) else { return };
        let program_files = std::env::var("ProgramW6432")
            .or_else(|_| std::env::var("ProgramFiles"))
            .unwrap_or_else(|_| r"C:\Program Files".into());
        // Old versions linger in the repository; keep one entry per family/verb.
        let mut seen: BTreeMap<(String, String, String), usize> = BTreeMap::new();
        for full_name in k.subkeys() {
            let Some(p) = k.sub(&full_name) else { continue };
            let root = p.get_str("PackageRootFolder").unwrap_or_default();
            let candidates = [
                format!("{root}\\AppxManifest.xml"),
                format!("{program_files}\\WindowsApps\\{full_name}\\AppxManifest.xml"),
            ];
            let Some((path, xml)) = candidates
                .iter()
                .filter(|c| !c.starts_with('\\'))
                .find_map(|c| std::fs::read(c).ok().map(|b| (c.clone(), String::from_utf8_lossy(&b).into_owned())))
            else {
                continue;
            };
            let verbs = parse_manifest(&xml);
            if verbs.verbs.is_empty() {
                continue;
            }
            let mut display = p.get_str("DisplayName").unwrap_or_default();
            if let Some(res) = display.strip_prefix("ms-resource:") {
                let name = full_name.split('_').next().unwrap_or("");
                let uri = if res.starts_with("//") {
                    format!("ms-resource:{res}")
                } else if res.starts_with('/') {
                    format!("ms-resource://{name}{res}")
                } else {
                    format!("ms-resource://{name}/Resources/{res}")
                };
                display = resolve_indirect(&format!("@{{{full_name}?{uri}}}"));
            }
            if display.is_empty() || display.contains("ms-resource:") {
                display = full_name.split('_').next().unwrap_or(&full_name).to_string();
            }
            let family = format!(
                "{}__{}",
                full_name.split('_').next().unwrap_or(""),
                full_name.rsplit("__").next().unwrap_or("")
            );
            for v in verbs.verbs {
                let clsid = format!("{{{}}}", v.clsid.trim_matches(|c| c == '{' || c == '}').to_ascii_uppercase());
                let scope = SCOPES
                    .iter()
                    .find(|(c, _)| c.eq_ignore_ascii_case(&v.item_type))
                    .map(|(_, l)| l.to_string())
                    .unwrap_or_else(|| v.item_type.clone());
                let blocked = self.is_blocked(&clsid);
                let dll = verbs.classes.get(&v.clsid.to_ascii_uppercase()).cloned().unwrap_or_default();
                let mut command = format!("[IExplorerCommand] {clsid}");
                if !dll.is_empty() {
                    command += &format!("  ->  {dll}");
                }
                let caption = format!("{display}: {}", v.id);
                let entry = Entry {
                    title: caption.clone(),
                    caption,
                    scope,
                    kind: Kind::Packaged,
                    status: if blocked { "Blocked".into() } else { String::new() },
                    hive: Hive::ClassesRoot,
                    key: path.clone(),
                    command,
                    caption_value: None,
                    command_type: None,
                    clsid: Some(clsid),
                    disabled: blocked,
                };
                // One row per verb: merge the item types it is registered for.
                let id = (family.clone(), v.id.clone(), entry.title.clone());
                match seen.get(&id) {
                    Some(&i) => {
                        let e = &mut self.out[i];
                        if !e.scope.split(", ").any(|s| s == entry.scope) {
                            e.scope = format!("{}, {}", e.scope, entry.scope);
                        }
                    }
                    None => {
                        seen.insert(id, self.out.len());
                        self.out.push(entry);
                    }
                }
            }
        }
    }
}

struct ManifestVerb {
    item_type: String,
    id: String,
    clsid: String,
}

#[derive(Default)]
struct Manifest {
    verbs: Vec<ManifestVerb>,
    /// CLSID (upper, no braces) -> DLL path from `com:Class`.
    classes: HashMap<String, String>,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')?;
    Some(tag[start..start + end].to_string())
}

/// Minimal tag scanner: enough for the `FileExplorerContextMenus` and `com:Class` elements.
fn parse_manifest(xml: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut in_menus = false;
    let mut item_type = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        let Some(len) = rest[start..].find('>') else { break };
        let tag = &rest[start + 1..start + len];
        rest = &rest[start + len + 1..];
        let closing = tag.starts_with('/');
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or(name);
        match local {
            "FileExplorerContextMenus" => in_menus = !closing,
            "ItemType" if in_menus && !closing => item_type = attr(tag, "Type").unwrap_or_default(),
            "Verb" if in_menus && !closing => {
                if let (Some(id), Some(clsid)) = (attr(tag, "Id"), attr(tag, "Clsid")) {
                    m.verbs.push(ManifestVerb { item_type: item_type.clone(), id, clsid });
                }
            }
            "Class" if !closing => {
                if let (Some(id), Some(path)) = (attr(tag, "Id"), attr(tag, "Path")) {
                    m.classes.insert(id.trim_matches(|c| c == '{' || c == '}').to_ascii_uppercase(), path);
                }
            }
            _ => {}
        }
    }
    m
}

fn blocked_clsids() -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for hive in [Hive::CurrentUser, Hive::LocalMachine] {
        let Some(k) = Key::open(hive, BLOCKED_KEY) else { continue };
        for name in k.value_names() {
            set.insert(name.to_ascii_uppercase());
        }
    }
    set
}

pub fn scan(include_file_types: bool) -> Vec<Entry> {
    let mut s = Scanner { out: Vec::new(), blocked: blocked_clsids(), clsid_cache: HashMap::new() };
    s.scan_packages();
    for hive in [Hive::CurrentUser, Hive::LocalMachine] {
        for (class, label) in SCOPES {
            s.scan_class(hive, class, label);
        }
    }
    if include_file_types {
        s.scan_file_types();
    }
    s.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_verbs() {
        let xml = r#"<Package><Extensions>
            <desktop4:Extension Category="windows.fileExplorerContextMenus">
              <desktop4:FileExplorerContextMenus>
                <desktop5:ItemType Type="Directory">
                  <desktop5:Verb Id="OpenTerminalHere" Clsid="9f156763-7844-4dc4-b2b1-901f640f5155" />
                </desktop5:ItemType>
                <desktop5:ItemType Type="*">
                  <desktop5:Verb Id="Edit" Clsid="E6950302-61F0-4FEB-97DB-855E30D4A991"/>
                </desktop5:ItemType>
              </desktop4:FileExplorerContextMenus>
            </desktop4:Extension>
            <com:Class Id="E6950302-61F0-4FEB-97DB-855E30D4A991" Path="NppShell.dll" ThreadingModel="STA"/>
            <desktop5:Verb Id="NotInMenus" Clsid="x"/>
        </Extensions></Package>"#;
        let m = parse_manifest(xml);
        assert_eq!(m.verbs.len(), 2);
        assert_eq!(m.verbs[0].item_type, "Directory");
        assert_eq!(m.verbs[0].id, "OpenTerminalHere");
        assert_eq!(m.verbs[1].item_type, "*");
        assert_eq!(m.classes["E6950302-61F0-4FEB-97DB-855E30D4A991"], "NppShell.dll");
    }

    #[test]
    fn strips_accelerators() {
        assert_eq!(strip_accel("&Open"), "Open");
        assert_eq!(strip_accel("Save && E&xit"), "Save & Exit");
    }

    #[test]
    fn scan_finds_something() {
        let entries = scan(false);
        assert!(entries.iter().any(|e| e.kind == Kind::Verb));
    }
}
