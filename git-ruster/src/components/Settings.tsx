import { useEffect, useState } from "preact/hooks";
import { api, Settings } from "../api";
import { refreshAll, setState, useAppState } from "../state";
import { toast, toastError } from "../ui";

export function SettingsDialog() {
  const s = useAppState();
  const [v, setV] = useState<Settings>(s.config!.settings);
  const [menuInstalled, setMenuInstalled] = useState<boolean | null>(null);

  useEffect(() => {
    api.contextMenuStatus().then(setMenuInstalled).catch(() => setMenuInstalled(false));
  }, []);

  const close = () => setState({ settingsOpen: false });
  const save = async () => {
    try {
      const cfg = await api.saveSettings({ ...v, maxCommits: Math.max(100, Number(v.maxCommits) || 3000), terminalFontSize: Math.max(8, Math.min(30, Number(v.terminalFontSize) || 13)) });
      const shellChanged = cfg.settings.shell !== s.config!.settings.shell;
      setState({ config: cfg, settingsOpen: false });
      refreshAll();
      if (shellChanged) toast("Shell change applies to newly started terminals (use Restart shell)");
    } catch (e) {
      toastError(e);
    }
  };
  const toggleMenu = async () => {
    try {
      if (menuInstalled) await api.contextMenuUninstall();
      else await api.contextMenuInstall();
      const now = await api.contextMenuStatus();
      setMenuInstalled(now);
      toast(now ? "Added “Open in Git Ruster” to the Explorer folder menu" : "Removed from the Explorer menu");
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <div class="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div class="dialog wide" onKeyDown={(e) => e.key === "Escape" && close()}>
        <div class="dialog-title">Settings</div>

        <label class="field">
          <span class="field-label">Terminal shell</span>
          <select value={v.shell} onChange={(e) => setV({ ...v, shell: (e.target as HTMLSelectElement).value as Settings["shell"] })}>
            <option value="auto">Auto (PowerShell 7 if installed, else Windows PowerShell)</option>
            <option value="pwsh">PowerShell 7 (pwsh)</option>
            <option value="powershell">Windows PowerShell 5.1</option>
            <option value="cmd">Command Prompt (cmd)</option>
            <option value="bash">Git Bash</option>
          </select>
        </label>
        <label class="field">
          <span class="field-label">Terminal font size</span>
          <input type="text" value={String(v.terminalFontSize)} onInput={(e) => setV({ ...v, terminalFontSize: Number((e.target as HTMLInputElement).value) })} />
        </label>
        <label class="field">
          <span class="field-label">Max commits in graph</span>
          <input type="text" value={String(v.maxCommits)} onInput={(e) => setV({ ...v, maxCommits: Number((e.target as HTMLInputElement).value) })} />
        </label>

        <div class="field">
          <span class="field-label">Windows Explorer integration</span>
          <div class="row-gap">
            <button onClick={toggleMenu} disabled={menuInstalled === null}>
              {menuInstalled ? "Remove “Open in Git Ruster” from folder menu" : "Add “Open in Git Ruster” to folder menu"}
            </button>
          </div>
          <span class="field-hint">
            Registered for the current user (HKCU\Software\Classes\Directory). On Windows 11 it appears under “Show more options”. Points to this executable — re-register after moving the app.
          </span>
        </div>

        <div class="dialog-buttons">
          <button onClick={close}>Cancel</button>
          <button class="primary" onClick={save}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
