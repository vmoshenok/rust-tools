import { api } from "../api";
import * as A from "../actions";
import { currentBranch, getState, isFiltered, openTerminal, refreshAll, repoView, setState, useAppState } from "../state";
import { showMenu, sep, toastError } from "../ui";
import { Icon } from "./Icon";

function Btn({ icon, label, title, onClick, onMenu, disabled, badge }: { icon: string; label: string; title?: string; onClick: () => void; onMenu?: (e: MouseEvent) => void; disabled?: boolean; badge?: number }) {
  return (
    <div class={`tb-btn-group ${disabled ? "disabled" : ""}`}>
      <button class="tb-btn" title={title ?? label} onClick={onClick} disabled={disabled}>
        <Icon name={icon} size={17} />
        <span>{label}</span>
        {badge ? <span class="tb-badge">{badge}</span> : null}
      </button>
      {onMenu && (
        <button class="tb-btn tb-arrow" onClick={(e) => onMenu(e)} disabled={disabled}>
          <Icon name="down" size={12} />
        </button>
      )}
    </div>
  );
}

export function Toolbar() {
  const s = useAppState();
  const info = s.info;
  const cb = currentBranch();
  const noRepo = !info;
  const menuAt = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { clientX: r.left, clientY: r.bottom + 2 };
  };

  const branchSwitcher = (e: MouseEvent) => {
    if (!info) return;
    showMenu(menuAt(e), [
      { label: "New branch…", action: () => A.createBranch() },
      sep,
      ...info.branches.map((b) => ({
        label: (b.current ? "✓ " : "    ") + b.name,
        hint: [b.ahead ? `↑${b.ahead}` : "", b.behind ? `↓${b.behind}` : ""].join(" ").trim() || undefined,
        action: () => !b.current && A.checkoutBranch(b.name),
      })),
    ]);
  };

  const repoName = s.repo ? s.repo.split(/[\\/]/).pop() : "";

  return (
    <div class="toolbar">
      <div class="tb-repo">
        <div class="tb-repo-name" title={s.repo ?? ""}>
          {repoName || "—"}
        </div>
        {info && (
          <button class="tb-branch" onClick={branchSwitcher} title="Switch branch">
            <Icon name="branch" size={13} />
            <span>{info.branch ?? `HEAD detached at ${info.headSha?.slice(0, 7) ?? "?"}`}</span>
            {cb && cb.ahead > 0 && <span class="track ahead">↑{cb.ahead}</span>}
            {cb && cb.behind > 0 && <span class="track behind">↓{cb.behind}</span>}
            <Icon name="down" size={11} />
          </button>
        )}
      </div>
      <div class="tb-actions">
        <Btn icon="fetch" label="Fetch" title="git fetch --all --prune" onClick={A.fetchAll} disabled={noRepo} />
        <Btn
          icon="pull"
          label="Pull"
          title="git pull (merge)"
          badge={cb?.behind}
          onClick={() => A.pull("merge")}
          disabled={noRepo}
          onMenu={(e) =>
            showMenu(menuAt(e), [
              { label: "Pull (merge)", action: () => A.pull("merge") },
              { label: "Pull (rebase)", action: () => A.pull("rebase") },
              { label: "Pull (fast-forward only)", action: () => A.pull("ff-only") },
            ])
          }
        />
        <Btn
          icon="push"
          label="Push"
          badge={cb?.ahead}
          onClick={() => A.push()}
          disabled={noRepo}
          onMenu={(e) =>
            showMenu(menuAt(e), [
              { label: "Push", action: () => A.push() },
              { label: "Force push (with lease)", action: () => A.push(true), danger: true },
              { label: "Push all tags", action: A.pushTags },
            ])
          }
        />
        <span class="tb-sep" />
        <Btn icon="branch" label="Branch" title="Create branch from HEAD" onClick={() => A.createBranch()} disabled={noRepo} />
        <Btn icon="stash" label="Stash" onClick={A.stashPush} disabled={noRepo || s.status.length === 0} />
        <Btn icon="pop" label="Pop" onClick={() => A.stashPop()} disabled={noRepo || !info?.stashes.length} />
        <span class="tb-sep" />
        <Btn
          icon="commit"
          label="Commit"
          title="Commit changes (Ctrl+K)"
          badge={s.status.length || undefined}
          onClick={() => setState({ selected: "WIP", focusCommitRequest: getState().focusCommitRequest + 1 })}
          disabled={noRepo}
        />
      </div>
      <div class="tb-right">
        <button
          class={`icon-btn ${s.info && (isFiltered() || Object.keys(repoView().priorities).length) ? "active" : ""}`}
          title="Filter / sort branches in graph"
          onClick={() => setState({ branchFilterOpen: true })}
          disabled={!s.info}
        >
          <Icon name="filter" size={17} />
        </button>
        <button class={`icon-btn ${s.terminalOpen ? "active" : ""}`} title="Terminal (Ctrl+`)" onClick={() => openTerminal(!s.terminalOpen)} disabled={!s.repo}>
          <Icon name="terminal" size={17} />
        </button>
        <button class={`icon-btn ${s.loading ? "spin" : ""}`} title="Refresh (F5)" onClick={() => refreshAll()} disabled={!s.repo}>
          <Icon name="refresh" size={17} />
        </button>
        <button class="icon-btn" title="Open in Explorer" onClick={() => s.repo && api.openInExplorer(s.repo).catch(toastError)} disabled={!s.repo}>
          <Icon name="explorer" size={17} />
        </button>
      </div>
    </div>
  );
}
