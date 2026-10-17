//! YAML configuration (`config.yml`).
//!
//! Lookup order:
//! 1. `config.yml` next to `rustshot.exe` (portable mode)
//! 2. `%APPDATA%\RustShot\config.yml` (created with defaults on first run)

use std::path::PathBuf;
use std::time::SystemTime;

use serde::Deserialize;

use crate::keys::{Binding, KeyCombo};
use crate::util;

/// Written to disk on first run. Must stay in sync with the `Default` impls.
pub const DEFAULT_YAML: &str = r##"# RustShot configuration.
# Changes are picked up automatically on the next capture
# (or immediately via tray menu -> "Reload settings").
#
# Key syntax: [Ctrl+][Alt+][Shift+][Win+]Key, several alternatives separated by ';'
# Keys: A-Z, 0-9, F1-F24, Esc, Enter, Space, Tab, Backspace, Delete, Insert,
#       Home, End, PageUp, PageDown, Up, Down, Left, Right, PrintScreen, Pause,
#       Plus, Minus, Comma, Period, Numpad0-Numpad9

Hotkeys:
  # Global hotkey(s) that start a capture (work from any application).
  MakeScreen: "PrintScreen;Ctrl+Shift+PrintScreen"

Keybindings:
  # Keys inside the drawing (editor) window.
  SaveToClipboard: "Esc;Enter;Ctrl+C"
  SaveToDefaultLocation: "Ctrl+S"
  CloseWithoutSaving: "F10"
  Undo: "Ctrl+Z"
  Redo: "Ctrl+Y;Ctrl+Shift+Z"
  ToolArrow: "A"
  ToolRectangle: "R"
  ToolText: "T"
  ToolPen: "P"
  # Move tool: click a shape to select it, drag to move, arrow keys nudge it.
  ToolMove: "H"
  DeleteSelected: "Delete;Backspace"
  # Keys 1..9 additionally pick a colour from the palette below.

# Folder for "SaveToDefaultLocation" (created if missing).
DefaultLocation: "c:\\temp"
# File name without extension. Tokens: {yyyy} {MM} {dd} {HH} {mm} {ss} {fff}
FileNamePattern: "Screenshot_{yyyy}-{MM}-{dd}_{HH}-{mm}-{ss}"
# Close the editor after saving to the default location.
CloseAfterSave: true
# Also put the image on the clipboard when saving to the default location.
CopyToClipboardOnSave: false
# Tray balloon notifications.
NotifyOnSave: true
NotifyOnCopy: false

Capture:
  # Click a window to capture it (otherwise only free selection).
  DetectWindows: true
  ShowMagnifier: true
  # 0.0 = no dimming, 1.0 = black, outside the selection.
  DimOpacity: 0.45
  # Delay before capturing when started from the tray menu (lets the menu fade out).
  TrayDelayMs: 250

Drawing:
  DefaultTool: "Arrow"          # Arrow | Rectangle | Text | Pen | Move
  Palette: ["#FF3B30", "#FF9500", "#FFCC00", "#34C759", "#0A84FF", "#AF52DE", "#000000", "#FFFFFF"]
  DefaultColor: 0               # index into Palette
  # Three size presets (S / M / L): stroke width and text size in pixels.
  LineWidths: [2, 4, 8]
  FontSizes: [16, 24, 36]
  DefaultSize: 1                # 0 = S, 1 = M, 2 = L
  FontFamily: "Segoe UI"
  Shadow: true                  # soft drop shadow under shapes
  # Text sits on a solid box in a contrasting colour (white behind dark text,
  # black behind light text). false = text with a thin outline only.
  TextBackground: true
"##;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase", default)]
pub struct Hotkeys {
    pub make_screen: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys { make_screen: "PrintScreen;Ctrl+Shift+PrintScreen".into() }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase", default)]
pub struct Keybindings {
    pub save_to_clipboard: String,
    pub save_to_default_location: String,
    pub close_without_saving: String,
    pub undo: String,
    pub redo: String,
    pub tool_arrow: String,
    pub tool_rectangle: String,
    pub tool_text: String,
    pub tool_pen: String,
    pub tool_move: String,
    pub delete_selected: String,
}

impl Default for Keybindings {
    fn default() -> Self {
        Keybindings {
            save_to_clipboard: "Esc;Enter;Ctrl+C".into(),
            save_to_default_location: "Ctrl+S".into(),
            close_without_saving: "F10".into(),
            undo: "Ctrl+Z".into(),
            redo: "Ctrl+Y;Ctrl+Shift+Z".into(),
            tool_arrow: "A".into(),
            tool_rectangle: "R".into(),
            tool_text: "T".into(),
            tool_pen: "P".into(),
            tool_move: "H".into(),
            delete_selected: "Delete;Backspace".into(),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase", default)]
pub struct CaptureOpts {
    pub detect_windows: bool,
    pub show_magnifier: bool,
    pub dim_opacity: f32,
    pub tray_delay_ms: u32,
}

impl Default for CaptureOpts {
    fn default() -> Self {
        CaptureOpts { detect_windows: true, show_magnifier: true, dim_opacity: 0.45, tray_delay_ms: 250 }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase", default)]
pub struct Drawing {
    pub default_tool: String,
    pub palette: Vec<String>,
    pub default_color: usize,
    pub line_widths: [f32; 3],
    pub font_sizes: [f32; 3],
    pub default_size: usize,
    pub font_family: String,
    pub shadow: bool,
    pub text_background: bool,
}

impl Default for Drawing {
    fn default() -> Self {
        Drawing {
            default_tool: "Arrow".into(),
            palette: ["#FF3B30", "#FF9500", "#FFCC00", "#34C759", "#0A84FF", "#AF52DE", "#000000", "#FFFFFF"]
                .map(String::from)
                .to_vec(),
            default_color: 0,
            line_widths: [2.0, 4.0, 8.0],
            font_sizes: [16.0, 24.0, 36.0],
            default_size: 1,
            font_family: "Segoe UI".into(),
            shadow: true,
            text_background: true,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase", default)]
pub struct Config {
    pub hotkeys: Hotkeys,
    pub keybindings: Keybindings,
    pub default_location: String,
    pub file_name_pattern: String,
    pub close_after_save: bool,
    pub copy_to_clipboard_on_save: bool,
    pub notify_on_save: bool,
    pub notify_on_copy: bool,
    pub capture: CaptureOpts,
    pub drawing: Drawing,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkeys: Hotkeys::default(),
            keybindings: Keybindings::default(),
            default_location: r"c:\temp".into(),
            file_name_pattern: "Screenshot_{yyyy}-{MM}-{dd}_{HH}-{mm}-{ss}".into(),
            close_after_save: true,
            copy_to_clipboard_on_save: false,
            notify_on_save: true,
            notify_on_copy: false,
            capture: CaptureOpts::default(),
            drawing: Drawing::default(),
        }
    }
}

/// Editor key bindings in parsed form.
#[derive(Clone, Debug, Default)]
pub struct EditorKeys {
    pub save_to_clipboard: Binding,
    pub save_to_default_location: Binding,
    pub close_without_saving: Binding,
    pub undo: Binding,
    pub redo: Binding,
    pub tool_arrow: Binding,
    pub tool_rectangle: Binding,
    pub tool_text: Binding,
    pub tool_pen: Binding,
    pub tool_move: Binding,
    pub delete_selected: Binding,
}

impl Config {
    pub fn editor_keys(&self, errors: &mut Vec<String>) -> EditorKeys {
        let k = &self.keybindings;
        let mut p = |s: &str| Binding::parse(s, errors);
        EditorKeys {
            save_to_clipboard: p(&k.save_to_clipboard),
            save_to_default_location: p(&k.save_to_default_location),
            close_without_saving: p(&k.close_without_saving),
            undo: p(&k.undo),
            redo: p(&k.redo),
            tool_arrow: p(&k.tool_arrow),
            tool_rectangle: p(&k.tool_rectangle),
            tool_text: p(&k.tool_text),
            tool_pen: p(&k.tool_pen),
            tool_move: p(&k.tool_move),
            delete_selected: p(&k.delete_selected),
        }
    }

    pub fn capture_hotkeys(&self, errors: &mut Vec<String>) -> Vec<KeyCombo> {
        Binding::parse(&self.hotkeys.make_screen, errors).0
    }

    /// Palette as 0xRRGGBB values (invalid entries are skipped).
    pub fn palette(&self) -> Vec<u32> {
        let v: Vec<u32> = self.drawing.palette.iter().filter_map(|s| parse_color(s)).collect();
        if v.is_empty() {
            vec![0xFF3B30]
        } else {
            v
        }
    }
}

pub fn parse_color(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    match h.len() {
        6 => u32::from_str_radix(h, 16).ok(),
        3 => {
            let v = u32::from_str_radix(h, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            Some((r * 17) << 16 | (g * 17) << 8 | (b * 17))
        }
        _ => None,
    }
}

pub fn config_path() -> PathBuf {
    let portable = util::exe_path().with_file_name("config.yml");
    if portable.exists() {
        return portable;
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join(util::APP_NAME).join("config.yml")
}

pub fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Load config, creating the default file if it does not exist.
/// On a parse error the defaults are returned together with the message.
pub fn load(path: &PathBuf) -> (Config, Option<String>) {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, DEFAULT_YAML);
    }
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_yaml::from_str::<Config>(&text) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(format!("{}: {e}", path.display()))),
        },
        Err(e) => (Config::default(), Some(format!("{}: {e}", path.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_yaml_matches_defaults() {
        let c: Config = serde_yaml::from_str(DEFAULT_YAML).unwrap();
        let d = Config::default();
        assert_eq!(c.keybindings.save_to_clipboard, d.keybindings.save_to_clipboard);
        assert_eq!(c.hotkeys.make_screen, d.hotkeys.make_screen);
        assert_eq!(c.keybindings.tool_move, d.keybindings.tool_move);
        assert_eq!(c.keybindings.delete_selected, d.keybindings.delete_selected);
        assert_eq!(c.default_location, d.default_location);
        assert_eq!(c.file_name_pattern, d.file_name_pattern);
        assert_eq!(c.drawing.palette, d.drawing.palette);
        assert_eq!(c.drawing.line_widths, d.drawing.line_widths);
        assert_eq!(c.drawing.text_background, d.drawing.text_background);
        let mut e = vec![];
        c.editor_keys(&mut e);
        c.capture_hotkeys(&mut e);
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn partial_yaml_uses_defaults() {
        let c: Config = serde_yaml::from_str("DefaultLocation: \"d:\\\\shots\"\n").unwrap();
        assert_eq!(c.default_location, r"d:\shots");
        assert_eq!(c.keybindings.close_without_saving, "F10");
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#FF3B30"), Some(0xFF3B30));
        assert_eq!(parse_color("fff"), Some(0xFFFFFF));
        assert_eq!(parse_color("nope"), None);
    }
}
