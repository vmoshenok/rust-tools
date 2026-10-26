// Background fetching: on startup the active repository first, then all other listed ones
// (so the sidebar shows where something new appeared); afterwards the active one periodically.
import { api } from "./api";
import { FetchState, getState, refreshAll, setState } from "./state";

const ACTIVE_INTERVAL = 30_000;
const PARALLEL = 4;

const EMPTY: FetchState = { fetching: false, behind: 0, fresh: false, error: null };

function patch(path: string, p: Partial<FetchState>) {
  const all = getState().fetchState;
  setState({ fetchState: { ...all, [path]: { ...EMPTY, ...all[path], ...p } } });
}

function repoPaths(): string[] {
  return getState().folders.flatMap((f) => (f.kind === "repo" ? [f.path] : f.kind === "group" ? f.children.map((c) => c.path) : []));
}

export async function fetchRepo(path: string) {
  if (getState().fetchState[path]?.fetching) return;
  patch(path, { fetching: true });
  try {
    const r = await api.fetchRepo(path);
    const active = getState().repo === path;
    patch(path, { fetching: false, behind: r.behind, error: null, fresh: !active && (r.changed || !!getState().fetchState[path]?.fresh) });
    if (active && r.changed) refreshAll();
  } catch (e) {
    patch(path, { fetching: false, error: String(e) });
  }
}

/** Fetches listed repositories that have not been fetched yet (all but `skip`), a few at a time. */
export async function fetchUnseen(skip?: string | null) {
  const queue = repoPaths().filter((p) => p !== skip && !getState().fetchState[p]);
  const worker = async () => {
    for (let p = queue.shift(); p; p = queue.shift()) await fetchRepo(p);
  };
  await Promise.all(Array.from({ length: PARALLEL }, worker));
}

let timer: number | undefined;
export async function startAutoFetch() {
  if (timer !== undefined) return;
  timer = window.setInterval(() => {
    const repo = getState().repo;
    if (repo && !getState().repoError) fetchRepo(repo);
  }, ACTIVE_INTERVAL);
  const active = getState().repo;
  if (active && !getState().repoError) await fetchRepo(active);
  await fetchUnseen(active);
}
