import { useMemo, useState } from "preact/hooks";
import { RepoView } from "../api";
import { repoView, saveRepoView, setState, useAppState } from "../state";
import { Icon } from "./Icon";

interface Item {
  ref: string; // full ref name
  name: string;
  kind: "local" | "remote";
  upstream?: string | null;
  current?: boolean;
}

/** Per-repository dialog: which branches the graph shows, and lane priorities. */
export function BranchFilterDialog() {
  const s = useAppState();
  const info = s.info!;
  const [v, setV] = useState<RepoView>(() => repoView());
  const [query, setQuery] = useState("");
  // Priorities are edited as text so the field can be empty.
  const [prio, setPrio] = useState<Record<string, string>>(() =>
    Object.fromEntries(Object.entries(repoView().priorities).map(([k, n]) => [k, String(n)])),
  );

  const items: Item[] = useMemo(
    () => [
      ...info.branches.map((b) => ({ ref: `refs/heads/${b.name}`, name: b.name, kind: "local" as const, upstream: b.upstream, current: b.current })),
      ...info.remoteBranches.map((r) => ({ ref: `refs/remotes/${r.name}`, name: r.name, kind: "remote" as const })),
    ],
    [info],
  );

  const hidden = new Set(v.hidden);
  // Remote branches kept by "Only local": upstreams of visible local branches.
  const keptUpstreams = new Set(info.branches.filter((b) => b.upstream && !hidden.has(`refs/heads/${b.name}`)).map((b) => b.upstream!));
  const blockedByOnlyLocal = (it: Item) => v.onlyLocal && it.kind === "remote" && !keptUpstreams.has(it.name);

  const q = query.trim().toLowerCase();
  const shown = items.filter((it) => !q || it.name.toLowerCase().includes(q));

  const setHidden = (refs: string[], hide: boolean) => {
    const next = new Set(v.hidden);
    refs.forEach((r) => (hide ? next.add(r) : next.delete(r)));
    setV({ ...v, hidden: [...next] });
  };

  const close = () => setState({ branchFilterOpen: false });
  const apply = () => {
    const priorities: Record<string, number> = {};
    for (const [ref, text] of Object.entries(prio)) {
      const n = parseInt(text, 10);
      if (n > 0 && items.some((it) => it.ref === ref)) priorities[ref] = n;
    }
    // Drop hidden entries for refs that no longer exist.
    const hiddenRefs = v.hidden.filter((h) => items.some((it) => it.ref === h));
    saveRepoView({ ...v, hidden: hiddenRefs, priorities });
    close();
  };
  const reset = () => {
    setV({ hidden: [], onlyLocal: false, hideTags: false, priorities: {} });
    setPrio({});
  };

  const visibleCount = items.filter((it) => !hidden.has(it.ref) && !blockedByOnlyLocal(it)).length;

  const section = (title: string, list: Item[]) =>
    list.length > 0 && (
      <>
        <div class="bf-section">{title}</div>
        {list.map((it) => {
          const blocked = blockedByOnlyLocal(it);
          return (
            <div key={it.ref} class={`bf-row ${blocked ? "blocked" : ""}`}>
              <label class="bf-name" title={blocked ? "Hidden by “Only local”" : it.ref}>
                <input type="checkbox" checked={!hidden.has(it.ref) && !blocked} disabled={blocked} onChange={(e) => setHidden([it.ref], !(e.target as HTMLInputElement).checked)} />
                <Icon name={it.kind === "remote" ? "cloud" : it.current ? "check" : "branch"} size={13} />
                <span class={it.current ? "bf-current" : ""}>{it.name}</span>
                {it.upstream && <span class="muted small">→ {it.upstream}</span>}
              </label>
              <input
                class="bf-prio"
                type="text"
                inputMode="numeric"
                placeholder="—"
                value={prio[it.ref] ?? ""}
                onInput={(e) => setPrio({ ...prio, [it.ref]: (e.target as HTMLInputElement).value.replace(/[^0-9]/g, "") })}
              />
            </div>
          );
        })}
      </>
    );

  return (
    <div class="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div
        class="dialog wide bf-dialog"
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          if (e.key === "Enter" && e.ctrlKey) apply();
        }}
      >
        <div class="dialog-title">Branches in graph — {s.repo?.split(/[\\/]/).pop()}</div>

        <div class="bf-options">
          <label class="check" title="Show only local branches and their upstream (remote-tracking) branches">
            <input type="checkbox" checked={v.onlyLocal} onChange={(e) => setV({ ...v, onlyLocal: (e.target as HTMLInputElement).checked })} />
            Only local <span class="muted">(+ their remote origins)</span>
          </label>
          <label class="check">
            <input type="checkbox" checked={!v.hideTags} onChange={(e) => setV({ ...v, hideTags: !(e.target as HTMLInputElement).checked })} />
            Show tags
          </label>
        </div>

        <div class="bf-toolbar">
          <span class="graph-search bf-search">
            <Icon name="search" size={13} />
            <input type="text" placeholder="Filter branches…" value={query} spellcheck={false} onInput={(e) => setQuery((e.target as HTMLInputElement).value)} />
          </span>
          <button onClick={() => setHidden(shown.map((i) => i.ref), false)}>Select all</button>
          <button onClick={() => setHidden(shown.map((i) => i.ref), true)}>Select none</button>
        </div>

        <div class="bf-head">
          <span>Branch</span>
          <span title="1 = leftmost lane in the graph. Empty = no priority (default layout).">Priority</span>
        </div>
        <div class="bf-list">
          {section("LOCAL", shown.filter((i) => i.kind === "local"))}
          {section("REMOTE", shown.filter((i) => i.kind === "remote"))}
          {shown.length === 0 && <div class="empty small">No branches match</div>}
        </div>

        <div class="field-hint">
          Priority: the branch with priority 1 is drawn in the leftmost lane, 2 in the next one, and so on (its first-parent history stays in that lane).
          Branches without a priority are laid out as usual to the right. The current branch is always shown.
        </div>

        <div class="dialog-buttons">
          <span class="muted small bf-count">
            {visibleCount} of {items.length} branches shown
          </span>
          <button onClick={reset}>Reset</button>
          <button onClick={close}>Cancel</button>
          <button class="primary" onClick={apply}>
            Apply
          </button>
        </div>
      </div>
    </div>
  );
}
