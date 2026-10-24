import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { api, CommitDetails, CommitFile, FileChange } from "../api";
import * as A from "../actions";
import { conflictLabels, DiffFile, setState, useAppState } from "../state";
import { showMenu, sep, toastError } from "../ui";
import { Icon } from "./Icon";
import { Avatar, formatDate, splitPath, statusInfo } from "./common";
import { RefBadge, laneColor } from "./Graph";

export function DetailsPanel() {
  const s = useAppState();
  if (!s.repo || s.repoError) return <div class="details" />;
  return <div class="details">{s.selected === "WIP" ? <WipPanel /> : s.selected ? <CommitPanel sha={s.selected} /> : null}</div>;
}

// ---------------------------------------------------------------------------------------------
// Commit details
// ---------------------------------------------------------------------------------------------

const detailsCache = new Map<string, CommitDetails>();

function commitDiffFiles(d: CommitDetails): DiffFile[] {
  const parent = d.parents[0];
  const short = d.sha.slice(0, 7);
  return d.files.map((f) => ({
    path: f.path,
    oldPath: f.oldPath,
    status: f.status,
    left: f.status === "A" || !parent ? { kind: "empty" } : { kind: "rev", rev: parent, path: f.oldPath ?? f.path },
    right: f.status === "D" ? { kind: "empty" } : { kind: "rev", rev: d.sha, path: f.path },
    leftLabel: parent ? `${parent.slice(0, 7)} (parent)` : "(none)",
    rightLabel: short,
  }));
}

function CommitPanel({ sha }: { sha: string }) {
  const s = useAppState();
  const repo = s.repo!;
  const key = repo + "|" + sha;
  const [d, setD] = useState<CommitDetails | null>(detailsCache.get(key) ?? null);
  const [sel, setSel] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const cached = detailsCache.get(key);
    setD(cached ?? null);
    setSel(null);
    if (!cached)
      api
        .commitDetails(repo, sha)
        .then((x) => {
          detailsCache.set(key, x);
          if (alive) setD(x);
        })
        .catch(toastError);
    return () => {
      alive = false;
    };
  }, [key]);

  if (!d) return <div class="details-loading muted">Loading…</div>;
  const row = s.graph?.rows.find((r) => r.sha === sha);
  const stash = s.info?.stashes.find((x) => x.sha === sha);
  const [subject, ...bodyLines] = d.message.split("\n");
  const body = bodyLines.join("\n").trim();
  const files = commitDiffFiles(d);
  const openDiff = (i: number) =>
    setState({ diff: { title: `${stash ? stash.name : "Commit " + sha.slice(0, 7)}`, files, index: i } });

  return (
    <div class="commit-panel">
      <div class="cp-header">
        <div class="cp-subject">{subject}</div>
        {row && row.refs.length > 0 && (
          <div class="cp-refs">
            {row.refs.map((r) => (
              <RefBadge key={r.kind + r.name} b={{ ref: r, remoteToo: false }} color={laneColor(row.color)} row={row} />
            ))}
          </div>
        )}
        {body && <pre class="cp-body">{body}</pre>}
      </div>
      <div class="cp-meta">
        <Avatar name={d.author} size={32} />
        <div>
          <div>
            <b>{d.author}</b> <span class="muted">&lt;{d.email}&gt;</span>
          </div>
          <div class="muted small">authored {formatDate(d.authorTime, true)}</div>
          {(d.committer !== d.author || d.commitTime !== d.authorTime) && (
            <div class="muted small">
              committed by {d.committer} {formatDate(d.commitTime, true)}
            </div>
          )}
        </div>
      </div>
      <div class="cp-ids">
        <span class="muted">commit</span>
        <span class="mono link" title="Copy" onClick={() => A.copy(d.sha)}>
          {d.sha.slice(0, 10)}
        </span>
        {d.parents.length > 0 && <span class="muted">parent{d.parents.length > 1 ? "s" : ""}</span>}
        {d.parents.map((p) => (
          <span class="mono link" title="Go to parent" onClick={() => setState({ selected: p, scrollTo: { sha: p, n: Date.now() } })}>
            {p.slice(0, 7)}
          </span>
        ))}
      </div>
      {stash && (
        <div class="cp-stash-actions">
          <button onClick={() => A.stashApply(stash.name)}>Apply</button>
          <button onClick={() => A.stashPop(stash.name)}>Pop</button>
          <button class="danger" onClick={() => A.stashDrop(stash.name)}>
            Drop
          </button>
        </div>
      )}
      <div class="cp-files-head">
        <span>
          {d.files.length} file{d.files.length === 1 ? "" : "s"} changed
        </span>
        <span class="muted small">double-click to view diff</span>
      </div>
      <div class="file-list">
        {d.files.map((f: CommitFile, i) => {
          const { name, dir } = splitPath(f.path);
          const st = statusInfo(f.status);
          return (
            <div
              key={f.path}
              class={`file-row ${sel === f.path ? "selected" : ""}`}
              onClick={() => setSel(f.path)}
              onDblClick={() => openDiff(i)}
              onKeyDown={(e) => e.key === "Enter" && openDiff(i)}
              tabIndex={0}
              onContextMenu={(e) => {
                setSel(f.path);
                showMenu(e, [
                  { label: "Show diff", action: () => openDiff(i), hint: "Enter" },
                  sep,
                  { label: "Copy path", action: () => A.copy(f.path) },
                  { label: "Show in Explorer", action: () => api.openInExplorer(`${repo}\\${f.path.replace(/\//g, "\\")}`).catch(toastError) },
                ]);
              }}
              title={f.oldPath ? `${f.oldPath} → ${f.path}` : f.path}
            >
              <span class={`st-letter ${st.cls}`}>{f.status}</span>
              <span class={`file-name ${st.cls}`}>{name}</span>
              <span class="file-dir">{f.oldPath ? `${f.oldPath} →` : dir}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------------------------
// Uncommitted changes / commit panel (JetBrains style)
// ---------------------------------------------------------------------------------------------

function wipDiffFile(f: FileChange, hasHead: boolean): DiffFile {
  const status = f.untracked ? "?" : f.conflicted ? "U" : f.y !== " " ? f.y : f.x;
  const added = f.untracked || f.x === "A" || !hasHead;
  const deleted = f.y === "D" || (f.x === "D" && f.y === " ");
  return {
    path: f.path,
    oldPath: f.origPath,
    status,
    left: added ? { kind: "empty" } : { kind: "rev", rev: "HEAD", path: f.origPath ?? f.path },
    right: deleted ? { kind: "empty" } : { kind: "work", path: f.path },
    leftLabel: "HEAD",
    rightLabel: "Working tree",
  };
}

const draftKey = (repo: string) => `git-ruster:draft:${repo}`;
function loadDraft(repo: string) {
  try {
    return localStorage.getItem(draftKey(repo)) ?? "";
  } catch {
    return "";
  }
}
function saveDraft(repo: string, msg: string) {
  try {
    if (msg) localStorage.setItem(draftKey(repo), msg);
    else localStorage.removeItem(draftKey(repo));
  } catch {
    /* ignore */
  }
}

function TriCheck({ state, onChange }: { state: "all" | "none" | "some"; onChange: (v: boolean) => void }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = state === "some";
  }, [state]);
  return <input ref={ref} type="checkbox" checked={state === "all"} onClick={(e) => e.stopPropagation()} onChange={() => onChange(state !== "all")} />;
}

function WipPanel() {
  const s = useAppState();
  const repo = s.repo!;
  const info = s.info;
  const files = s.status;
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [sel, setSel] = useState<string | null>(null);
  const [message, setMessageRaw] = useState(() => loadDraft(repo));
  const [amend, setAmend] = useState(false);
  const [lastMsg, setLastMsg] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const known = useRef(new Map<string, boolean>()); // path -> was conflicted
  const msgRef = useRef<HTMLTextAreaElement>(null);

  const setMessage = (m: string) => {
    setMessageRaw(m);
    saveDraft(repo, m);
  };

  // Reset when the repository changes.
  useEffect(() => {
    known.current = new Map();
    setChecked(new Set());
    setMessageRaw(loadDraft(repo));
    setAmend(false);
    setLastMsg(null);
  }, [repo]);

  // Keep check marks for known files, choose defaults for new ones.
  useEffect(() => {
    setChecked((prev) => {
      const next = new Set<string>();
      for (const f of files) {
        const wasConflicted = known.current.get(f.path);
        if (wasConflicted === undefined) {
          if (!f.untracked && !f.conflicted) next.add(f.path);
          else if (f.untracked && f.staged) next.add(f.path);
        } else if (wasConflicted && !f.conflicted) next.add(f.path); // just resolved
        else if (prev.has(f.path) && !f.conflicted) next.add(f.path);
        known.current.set(f.path, f.conflicted);
      }
      return next;
    });
  }, [files]);

  // Prefill message for merge / cherry-pick / revert.
  useEffect(() => {
    if (info && info.state !== "none" && !message) {
      api.readMergeMsg(repo).then((m) => m && setMessage(m)).catch(() => {});
    }
  }, [info?.state]);

  // Amend requested from the graph.
  useEffect(() => {
    if (s.amendRequest) toggleAmend(true);
  }, [s.amendRequest]);

  useEffect(() => {
    if (s.focusCommitRequest) setTimeout(() => msgRef.current?.focus(), 0);
  }, [s.focusCommitRequest]);

  async function toggleAmend(on: boolean) {
    setAmend(on);
    if (on) {
      try {
        const m = await api.lastCommitMessage(repo);
        setLastMsg(m);
        if (!message.trim() || message === lastMsg) setMessage(m);
      } catch (e) {
        toastError(e);
      }
      setTimeout(() => msgRef.current?.focus(), 0);
    } else if (lastMsg !== null && message === lastMsg) {
      setMessage("");
    }
  }

  const conflicts = files.filter((f) => f.conflicted);
  const smartStashed = A.smartStash();
  const trackedChanges = files.some((f) => !f.untracked && !f.conflicted);
  const changes = files.filter((f) => !f.conflicted && !f.untracked);
  const unversioned = files.filter((f) => f.untracked);
  const hasHead = !!info?.headSha;
  const diffFiles = useMemo(() => files.map((f) => wipDiffFile(f, hasHead)), [files, hasHead]);

  const openDiff = (path: string) => {
    const i = files.findIndex((f) => f.path === path);
    if (i >= 0) setState({ diff: { title: "Uncommitted changes", files: diffFiles, index: i } });
  };
  const openMerge = (path: string) => setState({ merge: { path } });

  const toggle = (path: string, on: boolean) =>
    setChecked((prev) => {
      const n = new Set(prev);
      on ? n.add(path) : n.delete(path);
      return n;
    });
  const groupState = (list: FileChange[]) => {
    const c = list.filter((f) => checked.has(f.path)).length;
    return c === 0 ? "none" : c === list.length ? "all" : "some";
  };
  const setGroup = (list: FileChange[], on: boolean) =>
    setChecked((prev) => {
      const n = new Set(prev);
      list.forEach((f) => (on ? n.add(f.path) : n.delete(f.path)));
      return n;
    });

  const checkedFiles = files.filter((f) => checked.has(f.path));
  const state = info?.state ?? "none";
  const doCommit = async (push: boolean) => {
    if (conflicts.length && state !== "none") {
      toastError(`Resolve ${conflicts.length} conflicted file(s) first`);
      return;
    }
    const ok = await A.commit({ files, checked, message, amend, push });
    if (ok) {
      setMessage("");
      setAmend(false);
      setLastMsg(null);
    }
  };

  const fileMenu = (e: MouseEvent, f: FileChange) => {
    setSel(f.path);
    const targets = checked.has(f.path) && checkedFiles.length > 1 ? checkedFiles : [f];
    const n = targets.length > 1 ? ` (${targets.length} files)` : "";
    showMenu(e, [
      ...(f.conflicted
        ? [
            { label: "Resolve conflict…", action: () => openMerge(f.path) },
            { label: `Accept Yours (${conflictLabels().ours})`, action: () => A.acceptSide(f, "ours") },
            { label: `Accept Theirs (${conflictLabels().theirs})`, action: () => A.acceptSide(f, "theirs") },
            { label: "Mark as resolved (git add)", action: () => A.markResolved(f.path) },
            sep,
          ]
        : []),
      { label: "Show diff", action: () => openDiff(f.path), hint: "Dbl-click" },
      sep,
      { label: `Stage (git add)${n}`, action: () => A.stageFiles(targets) },
      { label: `Unstage${n}`, action: () => A.unstageFiles(targets), disabled: !targets.some((t) => t.staged) },
      { label: `Rollback…${n}`, action: () => A.rollback(targets), danger: true },
      sep,
      { label: "Copy path", action: () => A.copy(f.path) },
      { label: "Show in Explorer", action: () => api.openInExplorer(`${repo}\\${f.path.replace(/\//g, "\\")}`).catch(toastError) },
    ]);
  };

  const group = (id: string, title: string, list: FileChange[], checkable: boolean) =>
    list.length > 0 && (
      <div class="wip-group">
        <div class="wip-group-head" onClick={() => setCollapsed((c) => ({ ...c, [id]: !c[id] }))}>
          <Icon name={collapsed[id] ? "right" : "down"} size={12} />
          {checkable && <TriCheck state={groupState(list)} onChange={(v) => setGroup(list, v)} />}
          <span>{title}</span>
          <span class="count">{list.length}</span>
        </div>
        {!collapsed[id] &&
          list.map((f) => {
            const { name, dir } = splitPath(f.path);
            const letter = f.untracked ? "?" : f.conflicted ? "U" : f.y !== " " ? f.y : f.x;
            const st = statusInfo(letter);
            return (
              <div
                key={f.path}
                class={`file-row ${sel === f.path ? "selected" : ""}`}
                tabIndex={0}
                onClick={() => setSel(f.path)}
                onDblClick={() => (f.conflicted ? openMerge(f.path) : openDiff(f.path))}
                onKeyDown={(e) => {
                  if (e.key === "Enter") f.conflicted ? openMerge(f.path) : openDiff(f.path);
                  if (e.key === " " && checkable) {
                    e.preventDefault();
                    toggle(f.path, !checked.has(f.path));
                  }
                }}
                onContextMenu={(e) => fileMenu(e, f)}
                title={`${st.label}${f.staged ? " (staged)" : ""}: ${f.origPath ? f.origPath + " → " : ""}${f.path}`}
              >
                {checkable ? (
                  <input type="checkbox" checked={checked.has(f.path)} onClick={(e) => e.stopPropagation()} onChange={(e) => toggle(f.path, (e.target as HTMLInputElement).checked)} />
                ) : (
                  <span class="cb-space" />
                )}
                <span class={`file-name ${st.cls}`}>{name}</span>
                <span class="file-dir">{f.origPath ? `${f.origPath} →` : dir}</span>
                {f.staged && <span class="staged-dot" title="Staged in index" />}
                {f.conflicted && (
                  <button class="small-btn" onClick={(e) => (e.stopPropagation(), openMerge(f.path))}>
                    Resolve…
                  </button>
                )}
              </div>
            );
          })}
      </div>
    );

  const firstLine = message.split("\n")[0];
  const commitLabel =
    state === "rebase" ? "Continue Rebase" : state === "merge" ? "Commit Merge" : amend ? "Amend Commit" : "Commit";

  return (
    <div class="wip-panel">
      <div class="wip-title">
        <span>Commit</span>
        <span class="spacer" />
        <button class="icon-btn" title="Rollback checked files…" disabled={!checkedFiles.length} onClick={() => A.rollback(checkedFiles)}>
          <Icon name="undo" size={15} />
        </button>
        <button class="icon-btn" title="Show diff" disabled={!sel} onClick={() => sel && openDiff(sel)}>
          <Icon name="diff" size={15} />
        </button>
        <button class="icon-btn" title="Stash changes…" disabled={!files.length} onClick={A.stashPush}>
          <Icon name="stash" size={15} />
        </button>
      </div>

      {state !== "none" && info && (
        <div class={`op-banner ${conflicts.length ? "has-conflicts" : ""}`}>
          <div class="op-banner-title">
            <Icon name={conflicts.length ? "warning" : "merge"} size={15} />
            <span>{info.stateLabel}</span>
          </div>
          <div class="op-banner-text">
            {conflicts.length
              ? `${conflicts.length} conflicted file${conflicts.length === 1 ? "" : "s"} — resolve them, then continue.`
              : "All conflicts resolved. Continue when ready."}
          </div>
          <div class="op-banner-buttons">
            {conflicts.length > 0 && (
              <button class="primary" onClick={() => openMerge(conflicts[0].path)}>
                Resolve conflicts…
              </button>
            )}
            {conflicts.length === 0 && state !== "merge" && (
              <button class="primary" onClick={A.continueOperation}>
                Continue
              </button>
            )}
            {(state === "rebase" || state === "cherry-pick" || state === "revert") && <button onClick={A.skipOperation}>Skip</button>}
            <button class="danger" onClick={A.abortOperation}>
              Abort
            </button>
          </div>
        </div>
      )}

      {state === "none" && smartStashed && (
        <div class={`op-banner ${conflicts.length ? "has-conflicts" : ""}`}>
          <div class="op-banner-title">
            <Icon name={conflicts.length ? "warning" : "stash"} size={15} />
            <span>{smartStashed.message.includes("smart pull") ? "Smart pull" : "Smart checkout"}</span>
          </div>
          <div class="op-banner-text">
            {conflicts.length
              ? `Unstashing your changes produced ${conflicts.length} conflict${conflicts.length === 1 ? "" : "s"}. Resolve them, then drop the stash (${smartStashed.name}).`
              : trackedChanges
                ? `Conflicts resolved. Your original changes are still saved in ${smartStashed.name} — drop it when you are happy with the result.`
                : `Your changes are still stashed in ${smartStashed.name} (the operation did not finish). Unstash them when ready.`}
          </div>
          <div class="op-banner-buttons">
            {conflicts.length > 0 && (
              <button class="primary" onClick={() => openMerge(conflicts[0].path)}>
                Resolve conflicts…
              </button>
            )}
            {!conflicts.length && !trackedChanges && (
              <button class="primary" onClick={() => A.stashPop(smartStashed.name)}>
                Unstash (pop)
              </button>
            )}
            <button class={conflicts.length || !trackedChanges ? "danger" : "primary"} onClick={() => A.stashDrop(smartStashed.name)}>
              Drop stash
            </button>
          </div>
        </div>
      )}

      <div class="wip-files">
        {files.length === 0 && <div class="empty small">No local changes</div>}
        {group("conflicts", "Merge Conflicts", conflicts, false)}
        {group("changes", "Changes", changes, true)}
        {group("unversioned", "Unversioned Files", unversioned, true)}
      </div>

      <div class="commit-box">
        <div class="commit-opts">
          <label class="check">
            <input type="checkbox" checked={amend} disabled={!hasHead || state === "rebase"} onChange={(e) => toggleAmend((e.target as HTMLInputElement).checked)} />
            Amend
          </label>
          <span class="spacer" />
          <span class={`muted small ${firstLine.length > 72 ? "warn" : ""}`}>{firstLine.length}</span>
        </div>
        <textarea
          ref={msgRef}
          class="commit-msg"
          placeholder={state === "rebase" ? "Message is kept by rebase --continue" : "Commit message"}
          value={message}
          spellcheck={false}
          onInput={(e) => setMessage((e.target as HTMLTextAreaElement).value)}
          onKeyDown={(e) => {
            if (e.ctrlKey && e.key === "Enter") {
              e.preventDefault();
              doCommit(e.shiftKey);
            }
          }}
        />
        <div class="commit-buttons">
          <button class="primary" onClick={() => doCommit(false)} title="Ctrl+Enter">
            {commitLabel}
          </button>
          {state !== "rebase" && (
            <button onClick={() => doCommit(true)} title="Ctrl+Shift+Enter">
              {commitLabel} and Push
            </button>
          )}
          <span class="spacer" />
          <span class="muted small">
            {checkedFiles.length}/{files.length - conflicts.length} selected
          </span>
        </div>
      </div>
    </div>
  );
}
