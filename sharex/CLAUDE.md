# Notes for AI assistants

- The spec is `REQUIREMENTS.md`; the user docs are `README.md`. When the user changes a
  requirement, update REQUIREMENTS.md (table + decisions), then the code, then the README.
- When adding a config key, update **both** `DEFAULT_YAML` and the `Default` impls
  in `src/config.rs`. The test `default_yaml_matches_defaults` guards this. Also update the
  README config block.
- This machine has no MSVC linker. Build and test with
  `cargo +stable-x86_64-pc-windows-gnu build|test`.
- Win32 via `windows-sys 0.59` (handles are raw pointers). Each window keeps its state in a
  `Box` stored in `GWLP_USERDATA`, freed in `WM_NCDESTROY`. Never destroy a window
  synchronously while holding `&mut State`; the editor hides and posts `WM_CLOSE` (`close()`).
- The process is Per-Monitor-V2 DPI aware: all coordinates are physical pixels.
- For a manual GUI test: run the exe, run it a second time to trigger a capture, then drive it
  with SetCursorPos/mouse_event/keybd_event from PowerShell.
