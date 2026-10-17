# RustShot — Requirements

The source of truth for what the tool must do. Update this file when the
requirements change, then update the code and README.

## Original request (2026-10-17)

> There is a nice program named ShareX or Snagit for making screenshots and
> drawing on them. I want the same mini program in Rust, for performance,
> with keybindings configured through a .yml file. I want a slightly adjusted
> user flow: first the user selects an area (like in ShareX: pick a window or
> drag a free selection with the mouse), then a drawing window opens where
> they can draw simple elements: arrows, boxes, text boxes and free-hand
> drawing. Pressing Esc or Enter (keybind action
> `"SaveToClipboard": "Esc;Enter;Ctrl+C"`) saves to the clipboard. Pressing
> `"SaveToDefaultLocation": "Ctrl+S"` with `"DefaultLocation": "c:\\temp"`
> saves a .png to the default location. Pressing `"CloseWithoutSaving": "F10"`
> just closes the window and does nothing. The program sits in the Windows
> tray with a nice icon (a PC monitor with a pencil). The tray menu has
> "Make screen" (to trigger a capture with the mouse) and a checkbox
> "Run at Windows startup". Write a README and save all requirements for
> later changes.

## Functional requirements

| ID | Requirement | Status |
|----|-------------|--------|
| R1 | Written in Rust, focused on speed: native Win32 + GDI/GDI+, no GUI framework | done |
| R2 | Runs in the Windows notification area (tray) with a custom icon: a PC monitor with a pencil | done (`src/icon_art.rs`, embedded by `build.rs`) |
| R3 | Tray menu: **Make screen**, **Run at Windows startup** (checkbox) | done (adds: Open screenshots folder, Open settings, Reload settings, Exit) |
| R4 | Run at startup = `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\RustShot` | done |
| R5 | Global hotkey starts a capture (configurable, default `PrintScreen`) | done |
| R6 | Step 1, area selection: hover and click picks a **window** (or a whole monitor over the desktop); drag picks a **free rectangle**; Esc / right click cancels | done |
| R7 | Step 2, drawing window with **arrow**, **rectangle (box)**, **text box**, **free-hand pen** | done |
| R8 | `SaveToClipboard` (default `Esc;Enter;Ctrl+C`): copy the annotated image to the clipboard and close | done |
| R9 | `SaveToDefaultLocation` (default `Ctrl+S`): save a .png to `DefaultLocation` (default `c:\temp`) | done (closes afterwards, see `CloseAfterSave`) |
| R10 | `CloseWithoutSaving` (default `F10`): close, do nothing | done |
| R11 | All keybindings live in a `.yml` file; syntax `Key;Ctrl+Key;...` | done (`config.yml`) |
| R12 | README plus this requirements file | done |
| R13 | Finished text can be edited again: click it with the Text tool, or double-click it with any tool. The cursor goes where you click; arrow keys, Home, End, Delete and Backspace work; undo/redo revert edits | done (2026-10-17) |
| R14 | Text has a contrasting background box for readability: white behind dark or saturated text (e.g. red), black behind light text (e.g. white, yellow) | done (`Drawing.TextBackground`, 2026-10-17) |
| R15 | Move (hand) tool: select an existing shape by clicking it, drag to move it without redrawing; `Del` removes the selected shape (`ToolMove: "H"`, `DeleteSelected: "Delete;Backspace"`); arrow keys nudge; moves and deletes can be undone | done (2026-10-17) |

## Decisions made during implementation (change if needed)

- **Config location**: `config.yml` next to the exe (portable) if it exists,
  otherwise `%APPDATA%\RustShot\config.yml`, created with defaults on first run.
  Changes are picked up automatically on the next capture.
- **PrintScreen** is caught with a low-level keyboard hook, so it works even
  when Windows 11 binds PrintScreen to the Snipping Tool. Other combos use
  `RegisterHotKey` and fall back to the hook if another app already has them.
- **Esc/Enter while typing text** finish the text box first. Pressing them
  again copies to the clipboard. Shift+Enter inserts a new line.
- After **Ctrl+S** the editor closes (`CloseAfterSave: true`), and a tray
  notification shows the path. Clicking it opens Explorer with the file selected.
- The clipboard gets both `CF_DIB` (24-bit) and `PNG` formats.
- Starting `rustshot.exe` while it is already running triggers a capture in
  the running instance. This is useful for mouse-button or desktop shortcuts.
- The editor window places the image exactly where it was captured when that
  fits, otherwise it centres it. Large captures are scaled down to fit the
  screen for viewing only; output is always full resolution.
- Undo/redo stores a snapshot of all shapes for each change, so edits to existing
  shapes (e.g. text) undo correctly.
- Extra editor features: palette colours (keys 1–9), 3 size presets, undo/redo,
  Shift for 45° arrows and square boxes, optional drop shadow.

## Ideas / backlog (not implemented)

- Select or move/resize existing shapes (including dragging a text box).
- More tools: highlighter, blur/pixelate, numbered step badges, ellipse, crop.
- Detect child controls (buttons, panels) during window selection, like ShareX.
- Delayed capture, capture of the active window or the full screen via separate hotkeys.
- "Save as…" dialog; JPG/WebP output; upload targets.
- Settings UI instead of editing YAML.
