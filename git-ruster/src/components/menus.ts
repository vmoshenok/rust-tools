// Context menus shared by the graph, sidebar and details panel.
import { GraphRow } from "../api";
import * as A from "../actions";
import { getState, repoView, saveRepoView, setState, toggleRefHidden } from "../state";
import { form } from "../ui";
import { revisionOf, showCompare, showCompareCommits, showCompareWith } from "../compare";
import { MenuItem, sep } from "../ui";

const cur = () => getState().info?.branch ?? "HEAD";

/** "Show diff with working tree" / "Show diff with…" for a revision. */
function diffItems(rev: string, label: string): MenuItem[] {
  return [
    { label: "Show diff with working tree", action: () => showCompare({ rev, label }, null) },
    { label: "Show diff with…", action: () => showCompareWith({ rev, label }) },
  ];
}

/** "Hide in graph" / "Graph priority…" entries for a branch (full ref name). */
function viewItems(fullRef: string, canHide = true): MenuItem[] {
  const v = repoView();
  const hidden = v.hidden.includes(fullRef);
  return [
    ...(canHide ? [{ label: hidden ? "Show in graph" : "Hide in graph", action: () => toggleRefHidden(fullRef) }] : []),
    { label: v.priorities[fullRef] ? `Graph priority… (#${v.priorities[fullRef]})` : "Graph priority…", action: () => setPriority(fullRef) },
    { label: "Filter / sort branches…", action: () => setState({ branchFilterOpen: true }) },
  ];
}

async function setPriority(fullRef: string) {
  const v = repoView();
  const res = await form({
    title: "Graph priority",
    message: `${fullRef.replace(/^refs\/(heads|remotes)\//, "")}: 1 = leftmost lane. Leave empty for no priority.`,
    fields: [{ name: "p", label: "Priority", type: "text", value: v.priorities[fullRef] ? String(v.priorities[fullRef]) : "" }],
    ok: "Set",
    validate: (x) => (x.p.trim() === "" || /^[1-9][0-9]*$/.test(x.p.trim()) ? null : "Enter a positive number"),
  });
  if (!res) return;
  const priorities = { ...v.priorities };
  if (res.p.trim()) priorities[fullRef] = parseInt(res.p, 10);
  else delete priorities[fullRef];
  saveRepoView({ ...v, priorities });
}

export function branchMenu(name: string): MenuItem[] {
  const info = getState().info;
  const b = info?.branches.find((x) => x.name === name);
  if (!b) return [];
  if (b.current)
    return [
      { label: "Pull", action: () => A.pull("merge") },
      { label: "Pull (rebase)", action: () => A.pull("rebase") },
      { label: "Push", action: () => A.push() },
      { label: "Force push (with lease)", action: () => A.push(true) },
      sep,
      { label: "New branch from here…", action: () => A.createBranch(b.sha, name) },
      { label: "Rename…", action: () => A.renameBranch(name) },
      sep,
      ...diffItems(name, name),
      { label: "Set upstream…", action: () => A.setUpstream(name), disabled: !info?.remoteBranches.length },
      sep,
      ...viewItems(`refs/heads/${name}`, false),
      sep,
      { label: "Copy branch name", action: () => A.copy(name) },
    ];
  return [
    { label: `Checkout ${name}`, action: () => A.checkoutBranch(name) },
    sep,
    { label: `Merge ${name} into ${cur()}…`, action: () => A.merge(name) },
    { label: `Rebase ${cur()} onto ${name}`, action: () => A.rebase(name) },
    sep,
    ...diffItems(name, name),
    sep,
    { label: `Push ${name}`, action: () => A.push(false, name) },
    { label: "New branch from here…", action: () => A.createBranch(b.sha, name) },
    { label: "Rename…", action: () => A.renameBranch(name) },
    { label: "Set upstream…", action: () => A.setUpstream(name), disabled: !info?.remoteBranches.length },
    { label: "Delete…", action: () => A.deleteBranch(name), danger: true },
    sep,
    ...viewItems(`refs/heads/${name}`),
    sep,
    { label: "Copy branch name", action: () => A.copy(name) },
  ];
}

export function remoteMenu(name: string): MenuItem[] {
  return [
    { label: `Checkout ${name.split("/").slice(1).join("/")} (track)`, action: () => A.checkoutRemote(name) },
    sep,
    { label: `Merge ${name} into ${cur()}…`, action: () => A.merge(name) },
    { label: `Rebase ${cur()} onto ${name}`, action: () => A.rebase(name) },
    sep,
    ...diffItems(name, name),
    sep,
    { label: "New branch from here…", action: () => A.createBranch(name, name) },
    { label: "Delete remote branch…", action: () => A.deleteRemoteBranch(name), danger: true },
    sep,
    ...viewItems(`refs/remotes/${name}`),
    sep,
    { label: "Copy branch name", action: () => A.copy(name) },
  ];
}

export function tagMenu(name: string, sha: string): MenuItem[] {
  return [
    { label: "Checkout tag (detached)", action: () => A.checkoutCommit(sha) },
    { label: `Merge ${name} into ${cur()}…`, action: () => A.merge(name) },
    { label: "New branch from tag…", action: () => A.createBranch(name, name) },
    sep,
    ...diffItems(name, name),
    sep,
    { label: "Push tag", action: () => A.pushTag(name) },
    { label: "Delete tag…", action: () => A.deleteTag(name), danger: true },
    sep,
    { label: "Copy tag name", action: () => A.copy(name) },
  ];
}

export function stashMenu(name: string): MenuItem[] {
  return [
    { label: "Apply stash", action: () => A.stashApply(name) },
    { label: "Pop stash", action: () => A.stashPop(name) },
    sep,
    { label: "Drop stash…", action: () => A.stashDrop(name), danger: true },
  ];
}

/** Commits reachable from HEAD by first-parent links (within the loaded graph). */
function firstParentChain(rows: GraphRow[], head: string | null): Set<string> {
  const bySha = new Map(rows.map((r) => [r.sha, r]));
  const chain = new Set<string>();
  let c = head ? bySha.get(head) : undefined;
  while (c && !chain.has(c.sha)) {
    chain.add(c.sha);
    c = c.parents[0] ? bySha.get(c.parents[0]) : undefined;
  }
  return chain;
}

export function commitMenu(row: GraphRow): MenuItem[] {
  const st = getState();
  const info = st.info;
  const short = row.sha.slice(0, 7);
  const isHead = row.head;
  const branches = row.refs.filter((r) => r.kind === "branch" && !r.current);
  const named = row.refs.find((r) => (r.kind === "branch" || r.kind === "remote") && !r.current)?.name;
  const target = named ?? row.sha;
  const targetLabel = named ?? short;
  const chain = firstParentChain(st.graph?.rows ?? [], info?.headSha ?? null);
  const busy = info?.state !== "none";
  const pair = st.compare && st.selected && st.selected !== "WIP" && (st.selected === row.sha || st.compare === row.sha) ? [st.selected, st.compare] : null;
  const rev = revisionOf(row.sha);

  return [
    ...(pair
      ? [
          { label: `Show diff ${pair[0].slice(0, 7)} ↔ ${pair[1].slice(0, 7)}`, action: () => showCompareCommits(pair[0], pair[1]) },
          sep,
        ]
      : []),
    ...branches.map((b) => ({ label: `Checkout ${b.name}`, action: () => A.checkoutBranch(b.name) })),
    ...(!isHead ? [{ label: "Checkout commit (detached)", action: () => A.checkoutCommit(row.sha) }] : []),
    sep,
    { label: "Create branch here…", action: () => A.createBranch(row.sha) },
    { label: "Create tag here…", action: () => A.createTag(row.sha) },
    sep,
    ...(!isHead
      ? [
          { label: `Merge ${targetLabel} into ${cur()}…`, action: () => A.merge(target, targetLabel), disabled: busy },
          { label: `Rebase ${cur()} onto ${targetLabel}`, action: () => A.rebase(target, targetLabel), disabled: busy },
          { label: "Cherry-pick commit", action: () => A.cherryPick(row), disabled: busy },
        ]
      : []),
    { label: "Revert commit", action: () => A.revert(row), disabled: busy },
    {
      label: `Reset ${cur()} to here`,
      disabled: busy && !isHead,
      submenu: [
        { label: "Soft (keep changes staged)", action: () => A.reset(row.sha, "soft") },
        { label: "Mixed (keep changes in working tree)", action: () => A.reset(row.sha, "mixed") },
        { label: "Hard (discard changes)", action: () => A.reset(row.sha, "hard"), danger: true },
      ],
    },
    ...(isHead
      ? [
          sep,
          { label: "Amend commit…", action: () => A.editHeadMessage() },
          { label: "Undo commit (soft reset)", action: () => row.parents[0] && A.run(["git", "reset", "--soft", row.parents[0]]), disabled: !row.parents[0] },
        ]
      : chain.has(row.sha)
        ? [sep, { label: `Squash ${short}..HEAD into one commit…`, action: () => A.squashInto(row) }]
        : []),
    sep,
    { label: "Show diff with working tree", action: () => showCompare(rev, null) },
    { label: "Show diff with…", action: () => showCompareWith(rev) },
    sep,
    { label: "Copy commit hash", action: () => A.copy(row.sha) },
    { label: "Copy subject", action: () => A.copy(row.subject) },
    { label: "Show details", action: () => setState({ selected: row.sha }) },
  ];
}
