import { open } from "@tauri-apps/plugin-dialog";
import { api, FolderNode } from "../api";
import { fetchUnseen } from "../autofetch";
import { getState, loadConfig, reloadFolders, repoView, selectRepo, setState, ui, updateUi, useAppState } from "../state";
import { showMenu, sep, toastError } from "../ui";
import * as A from "../actions";
import { Icon } from "./Icon";
import { branchMenu, remoteMenu, stashMenu, tagMenu } from "./menus";

export async function addFolderPaths(paths: string[]) {
  try {
    const cfg = await api.addFolders(paths);
    await loadConfig(cfg);
    fetchUnseen();
  } catch (e) {
    toastError(e);
  }
}

async function addFolderDialog() {
  const res = await open({ directory: true, multiple: true, title: "Add folder or repository" });
  if (!res) return;
  addFolderPaths(Array.isArray(res) ? res : [res]);
}

async function removeFolder(path: string) {
  try {
    const cfg = await api.removeFolder(path);
    setState({ config: cfg });
    await reloadFolders();
    if (getState().repo && !cfg.selected) {
      const st = getState();
      const stillListed = st.folders.some((f) => f.path === st.repo || f.children.some((c) => c.path === st.repo));
      if (!stillListed) selectRepo(null);
    }
  } catch (e) {
    toastError(e);
  }
}

function folderMenu(e: MouseEvent, f: FolderNode, repoPath?: string) {
  const path = repoPath ?? f.path;
  showMenu(e, [
    { label: "Open in Explorer", action: () => api.openInExplorer(path).catch(toastError) },
    { label: "Copy path", action: () => A.copy(path) },
    sep,
    ...(f.kind === "group" && !repoPath ? [{ label: "Rescan for repositories", action: reloadFolders }] : []),
    ...(f.kind === "plain" ? [{ label: "Initialize git repository", action: () => selectRepo(f.path).then(A.initRepo) }] : []),
    ...(!repoPath ? [{ label: "Remove from list", action: () => removeFolder(f.path), danger: true }] : []),
  ]);
}

function Section({ id, title, count, children, actions }: { id: string; title: string; count?: number; children: any; actions?: any }) {
  const collapsed = ui().collapsed?.[id] ?? false;
  const toggle = () => updateUi({ collapsed: { ...(ui().collapsed ?? {}), [id]: !collapsed } });
  return (
    <div class="side-section">
      <div class="side-section-head" onClick={toggle}>
        <Icon name={collapsed ? "right" : "down"} size={12} />
        <span>{title}</span>
        {count !== undefined && <span class="count">{count}</span>}
        <span class="spacer" />
        {actions && <span onClick={(e) => e.stopPropagation()}>{actions}</span>}
      </div>
      {!collapsed && <div class="side-section-body">{children}</div>}
    </div>
  );
}

/** Result of the background fetch: commits behind upstream, "new" dot, error mark. */
function FetchBadge({ path }: { path: string }) {
  const f = getState().fetchState[path];
  if (!f) return null;
  return (
    <>
      {f.fresh && <span class="fetch-new" title="New commits or branches fetched since you last opened it" />}
      {f.behind > 0 && (
        <span class="track behind" title={`Local branches are ${f.behind} commit${f.behind === 1 ? "" : "s"} behind their upstreams`}>
          ↓{f.behind}
        </span>
      )}
      {f.error && (
        <span class="track gone" title={`Background fetch failed:\n${f.error}`}>
          !
        </span>
      )}
    </>
  );
}

function scrollToSha(sha: string) {
  setState({ selected: sha, scrollTo: { sha, n: Date.now() } });
}

export function Sidebar() {
  const s = useAppState();
  const info = s.info;
  const isSel = (p: string) => s.repo === p;
  const view = repoView();
  const hiddenRefs = new Set(view.hidden);
  const keptUpstreams = new Set(info?.branches.filter((b) => b.upstream && !hiddenRefs.has(`refs/heads/${b.name}`)).map((b) => b.upstream!) ?? []);
  const isHidden = (ref: string, remoteName?: string) =>
    hiddenRefs.has(ref) || (!!remoteName && view.onlyLocal && !keptUpstreams.has(remoteName));
  const prioOf = (ref: string) => view.priorities[ref];

  // Remote branches grouped by remote name.
  const remotes = new Map<string, { name: string; sha: string }[]>();
  info?.remoteBranches.forEach((r) => {
    const [remote] = r.name.split("/");
    if (!remotes.has(remote)) remotes.set(remote, []);
    remotes.get(remote)!.push(r);
  });

  return (
    <div class="sidebar">
      <div class="side-top">
        <div class="side-repos">
          <Section
            id="repos"
            title="REPOSITORIES"
            actions={
              <button class="icon-btn" title="Add folder…" onClick={addFolderDialog}>
                <Icon name="plus" size={14} />
              </button>
            }
          >
            {s.folders.length === 0 && (
              <div class="side-empty">
                <button onClick={addFolderDialog}>
                  <Icon name="plus" size={14} /> Add folder
                </button>
              </div>
            )}
            {s.folders.map((f) => (
              <div key={f.path}>
                <div
                  class={`side-item ${f.kind !== "group" && isSel(f.path) ? "selected" : ""} ${f.kind}`}
                  title={f.path}
                  onClick={() => f.kind !== "group" && f.kind !== "missing" && selectRepo(f.path)}
                  onContextMenu={(e) => folderMenu(e, f)}
                >
                  <Icon name={f.kind === "repo" ? "repo" : "folder"} size={14} />
                  <span class="name">{f.name}</span>
                  {f.kind === "missing" && <span class="muted"> (missing)</span>}
                  {f.kind === "repo" && <FetchBadge path={f.path} />}
                  {f.kind === "group" && <span class="count">{f.children.length}</span>}
                </div>
                {f.kind === "group" &&
                  f.children.map((c) => (
                    <div
                      key={c.path}
                      class={`side-item child ${isSel(c.path) ? "selected" : ""}`}
                      title={c.path}
                      onClick={() => selectRepo(c.path)}
                      onContextMenu={(e) => folderMenu(e, f, c.path)}
                    >
                      <Icon name="repo" size={14} />
                      <span class="name">{c.name}</span>
                      <FetchBadge path={c.path} />
                    </div>
                  ))}
              </div>
            ))}
          </Section>
        </div>
        {info && (
          <div class="side-refs">
            <Section id="local" title="LOCAL BRANCHES" count={info.branches.length}>
              {info.branches.map((b) => (
                <div
                  key={b.name}
                  class={`side-item ref ${b.current ? "current" : ""} ${s.selected === b.sha ? "selected-soft" : ""} ${isHidden(`refs/heads/${b.name}`) && !b.current ? "hidden-ref" : ""}`}
                  onClick={() => scrollToSha(b.sha)}
                  onDblClick={() => !b.current && A.checkoutBranch(b.name)}
                  onContextMenu={(e) => showMenu(e, branchMenu(b.name))}
                  title={b.upstream ? `${b.name} → ${b.upstream}` : b.name}
                >
                  <Icon name={b.current ? "check" : "branch"} size={13} />
                  <span class="name">{b.name}</span>
                  {prioOf(`refs/heads/${b.name}`) && <span class="prio" title="Graph lane priority">#{prioOf(`refs/heads/${b.name}`)}</span>}
                  {b.gone && <span class="track gone">gone</span>}
                  {b.ahead > 0 && <span class="track ahead">↑{b.ahead}</span>}
                  {b.behind > 0 && <span class="track behind">↓{b.behind}</span>}
                </div>
              ))}
            </Section>
            <Section id="remote" title="REMOTES" count={info.remoteBranches.length}>
              {[...remotes.entries()].map(([remote, list]) => (
                <div key={remote}>
                  <div class="side-item sub-head">
                    <Icon name="cloud" size={13} />
                    <span class="name">{remote}</span>
                  </div>
                  {list.map((r) => (
                    <div
                      key={r.name}
                      class={`side-item ref child ${s.selected === r.sha ? "selected-soft" : ""} ${isHidden(`refs/remotes/${r.name}`, r.name) ? "hidden-ref" : ""}`}
                      onClick={() => scrollToSha(r.sha)}
                      onDblClick={() => A.checkoutRemote(r.name)}
                      onContextMenu={(e) => showMenu(e, remoteMenu(r.name))}
                      title={r.name}
                    >
                      <Icon name="branch" size={13} />
                      <span class="name">{r.name.slice(remote.length + 1)}</span>
                      {prioOf(`refs/remotes/${r.name}`) && <span class="prio" title="Graph lane priority">#{prioOf(`refs/remotes/${r.name}`)}</span>}
                    </div>
                  ))}
                </div>
              ))}
            </Section>
            <Section id="tags" title="TAGS" count={info.tags.length}>
              {[...info.tags].reverse().map((t) => (
                <div
                  key={t.name}
                  class={`side-item ref ${s.selected === t.sha ? "selected-soft" : ""}`}
                  onClick={() => scrollToSha(t.sha)}
                  onContextMenu={(e) => showMenu(e, tagMenu(t.name, t.sha))}
                >
                  <Icon name="tag" size={13} />
                  <span class="name">{t.name}</span>
                </div>
              ))}
            </Section>
            <Section id="stashes" title="STASHES" count={info.stashes.length}>
              {info.stashes.map((st) => (
                <div
                  key={st.name}
                  class={`side-item ref ${s.selected === st.sha ? "selected" : ""}`}
                  title={`${st.name}: ${st.message}`}
                  onClick={() => setState({ selected: st.sha })}
                  onDblClick={() => A.stashApply(st.name)}
                  onContextMenu={(e) => showMenu(e, stashMenu(st.name))}
                >
                  <Icon name="stash" size={13} />
                  <span class="name">{st.message}</span>
                </div>
              ))}
            </Section>
          </div>
        )}
      </div>
      <div class="side-bottom">
        <button class="icon-btn" title="Settings" onClick={() => setState({ settingsOpen: true })}>
          <Icon name="settings" size={16} />
        </button>
        <span class="muted small">Git Ruster</span>
      </div>
    </div>
  );
}
