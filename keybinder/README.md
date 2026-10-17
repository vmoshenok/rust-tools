# Keybinder

A tiny Windows tray app. Press a hotkey and it types a text string you configured, as if you typed it on the keyboard.

- Single exe of about 420 KB that uses about 10 MB of RAM while idle.
- Uses only `serde_yaml` and `windows-sys`. There's no GUI framework; it calls the Win32 API directly.

## Usage

Run `keybinder.exe`. A keyboard icon appears in the system tray. On first run the app creates `keybinder.yml` **next to the exe**:

```yaml
macros:
  "Ctrl+Shift+P": "some_looong_string"
  "Ctrl+Alt+E": "me@example.com"
  "Win+F2": "line one\nline two"
```

- **Modifiers:** `Ctrl`, `Shift`, `Alt`, `Win`. `Control`, `Super` and `Meta` also work.
- **Keys:** `A`–`Z`, `0`–`9`, `F1`–`F24`, `Space`, `Enter`, `Tab`, `Esc`, `Backspace`, `Insert`, `Delete`, `Home`, `End`, `PageUp`, `PageDown`, `Up`, `Down`, `Left`, `Right`, `Num0`–`Num9`, `Pause`, `PrintScreen`, or a single punctuation character. For a punctuation key, the app looks up which key types it in the current keyboard layout. `Ctrl++` means Ctrl and the plus key.
- **Text:** in double-quoted YAML, `\n` presses Enter and `\t` presses Tab. Every other character is sent as Unicode, so any language works whatever the keyboard layout.
- **Flat files:** you can leave out the top-level `macros:` key and list the hotkeys directly at the top level.

Tray menu (left- or right-click the icon):
- **Edit config** opens the file in the default `.yml` editor, or Notepad if there isn't one.
- **Reload config** applies your edits without restarting.
- **Run at Windows startup** is a checkbox for autostart.
- **Exit**

## Building

```
cargo build --release
```

Output: `target\release\keybinder.exe`.

`rust-toolchain.toml` pins the **GNU** toolchain (`stable-x86_64-pc-windows-gnu`). This machine has no Visual Studio Build Tools, so it has no MSVC `link.exe`. The GNU toolchain includes its own linker. If you install MSVC Build Tools, you can delete `rust-toolchain.toml` to use MSVC instead.

The release profile is tuned for size: `opt-level = "z"`, LTO, `panic = "abort"`, stripped symbols.

## Code map (`src/main.rs`)

All the code is in one file, split into these sections:

| Section | What it does |
|---|---|
| config | `load_config()` reads and parses the YAML. `parse_hotkey()` and `parse_key()` turn `"Ctrl+Shift+P"` into the modifiers and virtual-key code. `apply_config()` unregisters the old hotkeys and registers the new ones. |
| typing | `type_text()` waits until the user releases the hotkey modifiers, then sends the text with `SendInput`. |
| autostart | Reads, writes or deletes the value `Keybinder` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. |
| icon | `render_icon()` draws the keyboard as pixels from the `KEYBOARD` shape list. `create_icon()` wraps those pixels into an `HICON`. |
| tray | Adds the tray icon, updates its tooltip, shows the popup menu and opens the config in an editor. |
| `wndproc` / `main` | The hidden window that receives every message, plus the single-instance check, DPI setup and message loop. |

## Design notes / gotchas

- **Hotkeys** are registered with `RegisterHotKey` + `MOD_NOREPEAT`. Hotkey id `N` maps to `MACROS[N-1]`, and only hotkeys that registered successfully are stored, so ids stay contiguous. If a hotkey fails to register (usually because another app owns it) or can't be parsed, it's reported in a message box and skipped.
- **Modifier wait:** when a hotkey fires, Ctrl/Shift/etc. are still held down. If the app typed right away, the text would arrive as shortcuts such as Ctrl+S. So `type_text` polls `GetAsyncKeyState` for up to about 2 s, waiting until the modifiers are released. It runs on its own thread so the tray menu stays responsive.
- **Text is sent as Unicode:** `KEYEVENTF_UNICODE` sends characters rather than key presses, so the typed text doesn't depend on the keyboard layout. `\n` and `\t` are the exception: they're sent as real Enter and Tab key presses, because many apps ignore Unicode newlines.
- **Hidden window, not message-only:** the window is a normal top-level window that is never shown. A message-only window (`HWND_MESSAGE`) wouldn't receive the `TaskbarCreated` broadcast, which the app needs to re-add its icon after Explorer restarts.
- **Config next to the exe:** the config path comes from `current_exe()`, not the working directory. Apps launched at startup run with `System32` as their working directory.
- **Autostart stays correct if you move the exe:** at launch, if the startup entry exists, it's rewritten with the exe's current path.
- **Single instance:** a named mutex, `Local\KeybinderSingleInstance`, prevents a second copy from running.
- **DPI:** the app declares itself DPI-aware (`PER_MONITOR_AWARE_V2`), so the icon is drawn at `SM_CXSMICON` pixels: 16 at 100% scaling, 24 at 150%, 32 at 200%. Shape edges are snapped to whole pixels so the icon stays sharp at in-between scales.
- **Icon edits:** the shapes are rounded rectangles on a 16×16 grid in `KEYBOARD`, and the colors are the constants `BODY`, `KEY` and `ACCENT`. Shapes later in the list are drawn on top.
- **Admin windows:** because of Windows UIPI, the app can't type into windows running as administrator unless keybinder also runs as administrator.

## Possible future work

- An icon for the exe file in Explorer. This needs a Windows resource compiled into the exe. The GNU toolchain doesn't include `windres`, so you'd need MSVC `rc.exe`, a mingw install, or a hand-written COFF `.rsrc` object in `build.rs`.
- Watch `keybinder.yml` and reload automatically when it changes.
- Paste through the clipboard for very long strings, which would be faster than simulating keystrokes.
- Macros that press real key combos rather than typing text.
