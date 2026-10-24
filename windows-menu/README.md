# windows-menu — Windows context menu editor

A small native Win32 tool (Rust + `windows-sys`, no UI framework) that lists every
Explorer right-click menu item, shows **where it is registered**, and lets you edit it.

## Where do context menu items come from?

| What you see | Where it lives | Editable here |
|---|---|---|
| Windows 11 top-level menu (Open in Terminal, Edit in Notepad, Open with Code, ...) | `AppxManifest.xml` of a packaged / sparse-packaged app, `windows.fileExplorerContextMenus` extension (an `IExplorerCommand` COM class). The package list is in `HKCR\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages`. | Block / unblock only (manifest is signed) |
| "Show more options" — static verbs | `HKCR\<class>\shell\<verb>`: caption = `MUIVerb` or the key's default value, command = `<verb>\command` default value. | Caption, command, disable/enable |
| "Show more options" — shell extensions (7-Zip, WinRAR, Defender scan, ...) | `HKCR\<class>\shellex\ContextMenuHandlers\<name>` → `{CLSID}` → `HKCR\CLSID\{CLSID}\InprocServer32` (DLL). Text is produced by the DLL at runtime. | Block / unblock only |
| Cut, Copy, Rename, Delete, Properties, ... | Built into Explorer | No |

`HKCR` is a merged view of `HKCU\Software\Classes` (per user, wins) and
`HKLM\Software\Classes` (machine). The tool shows the real key, so you know which one to edit.

Common `<class>` values: `*` (all files), `AllFilesystemObjects`, `Directory` (folders),
`Directory\Background` (empty space in a folder), `Drive`, `DesktopBackground`, `Folder`.
Per-file-type items live under the ProgID of the extension (`.txt` → `txtfile\shell`) and
`SystemFileAssociations\.ext\shell` — tick **Include per-file-type entries** to see them.

## Usage

- **Filter** searches all columns; click a column header to sort.
- Select a row to see its caption, command and registry path below.
- **Save** (or Enter in the caption/command box) writes the change.
- **Disable / Enable** (verbs) toggles the `LegacyDisable` value.
- **Block / Unblock** (shell extensions, Windows 11 items) adds/removes the CLSID in
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked`.
  Restart Explorer for this to take effect. Note it blocks that COM class everywhere.
- **Open in Regedit** (or double-click) opens regedit at the key; for Windows 11 items it
  shows the manifest in Explorer.
- Status flags: *Shift+right-click only* (`Extended`), *Hidden (programmatic)*
  (`ProgrammaticAccessOnly`), *Conditional* (`AppliesTo`).

Items under `HKLM` need administrator rights to change; on "Access denied" the tool offers
to restart itself elevated.

## Build

```
cargo build --release
```

Uses the GNU toolchain (`rust-toolchain.toml`) because MSVC Build Tools aren't installed.
`build.rs` embeds the application manifest (Common Controls v6, DPI aware) without rc.exe.
