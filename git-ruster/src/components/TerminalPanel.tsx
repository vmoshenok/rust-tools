import { useEffect, useRef } from "preact/hooks";
import { openTerminal, useAppState } from "../state";
import { attachTerminal, clearTerminal, fitTerminal, restartTerminal } from "../terminal";
import { Icon } from "./Icon";

export function TerminalPanel() {
  const s = useAppState();
  const host = useRef<HTMLDivElement>(null);
  const repo = s.repo!;

  useEffect(() => {
    if (host.current) attachTerminal(repo, repo, host.current);
  }, [repo]);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const ro = new ResizeObserver(() => fitTerminal(repo));
    ro.observe(el);
    return () => ro.disconnect();
  }, [repo]);

  return (
    <div class="terminal-panel">
      <div class="terminal-head">
        <Icon name="terminal" size={14} />
        <span>Terminal</span>
        <span class="muted small">{repo}</span>
        <span class="spacer" />
        <button class="icon-btn" title="Clear" onClick={() => clearTerminal(repo)}>
          <Icon name="x" size={13} />
        </button>
        <button class="icon-btn" title="Restart shell" onClick={() => restartTerminal(repo)}>
          <Icon name="refresh" size={13} />
        </button>
        <button class="icon-btn" title="Hide (Ctrl+`)" onClick={() => openTerminal(false)}>
          <Icon name="down" size={14} />
        </button>
      </div>
      <div class="terminal-body" ref={host} />
    </div>
  );
}
