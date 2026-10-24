// Comparing two revisions, or a revision with the working tree, in the diff viewer.
import { api } from "./api";
import { DiffFile, getState, Revision, setState } from "./state";
import { toast, toastError } from "./ui";

export const shortRev = (rev: string) => (/^[0-9a-f]{40}$/.test(rev) ? rev.slice(0, 7) : rev);

/** Label for a commit: its first branch/tag name if any, else the short sha. */
export function revisionOf(sha: string): Revision {
  const row = getState().graph?.rows.find((r) => r.sha === sha);
  const ref = row?.refs.find((r) => r.kind !== "head");
  return { rev: sha, label: ref ? `${ref.name} (${sha.slice(0, 7)})` : sha.slice(0, 7) };
}

/** Opens the diff viewer for `left` → `right` (`right` null = working tree). */
export async function showCompare(left: Revision, right: Revision | null) {
  const repo = getState().repo;
  if (!repo) return;
  try {
    const files = await api.compareFiles(repo, left.rev, right?.rev ?? null);
    if (!files.length) return toast(`No differences between ${left.label} and ${right?.label ?? "the working tree"}`);
    const rightLabel = right?.label ?? "Working tree";
    const diffFiles: DiffFile[] = files.map((f) => ({
      path: f.path,
      oldPath: f.oldPath,
      status: f.status,
      left: f.status === "A" ? { kind: "empty" } : { kind: "rev", rev: left.rev, path: f.oldPath ?? f.path },
      right: f.status === "D" ? { kind: "empty" } : right ? { kind: "rev", rev: right.rev, path: f.path } : { kind: "work", path: f.path },
      leftLabel: left.label,
      rightLabel,
    }));
    setState({ diff: { title: `${left.label} ↔ ${rightLabel}`, files: diffFiles, index: 0 } });
  } catch (e) {
    toastError(e);
  }
}

/** Diff between two commits of the graph, older one on the left. */
export function showCompareCommits(a: string, b: string) {
  const rows = getState().graph?.rows ?? [];
  const ia = rows.findIndex((r) => r.sha === a);
  const ib = rows.findIndex((r) => r.sha === b);
  // Rows are newest first: the larger index is the older commit.
  const [older, newer] = ia > ib ? [a, b] : [b, a];
  return showCompare(revisionOf(older), revisionOf(newer));
}

/** Asks the user for a revision (branch, tag, commit). */
export function pickRevision(title: string): Promise<Revision | null> {
  return new Promise((resolve) => setState({ picker: { title, resolve } }));
}

export async function showCompareWith(base: Revision) {
  const other = await pickRevision(`Show diff of ${base.label} with…`);
  if (other) showCompare(base, other);
}
