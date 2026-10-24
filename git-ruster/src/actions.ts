// Git operations. Every repository-changing command is typed into the built-in terminal,
// so the user always sees exactly what runs (and can answer prompts, e.g. credentials).
import { api, FileChange, GraphRow } from "./api";
import { currentBranch, getState, openTerminal, refreshAll, setState, SMART_STASH_PREFIX } from "./state";
import { markRunning } from "./terminal";
import { confirm, form, toast, toastError } from "./ui";

type Cmd = string[];

function repo(): string {
  const r = getState().repo;
  if (!r) throw new Error("No repository selected");
  return r;
}

function hash(s: string) {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return (h >>> 0).toString(16);
}

/** Runs commands (chained with &&) in the repository terminal. */
export async function run(...cmds: Cmd[]) {
  try {
    const r = repo();
    openTerminal(true);
    await new Promise((res) => requestAnimationFrame(() => res(null)));
    markRunning(r);
    await api.termRun(r, r, cmds);
  } catch (e) {
    toastError(e);
  }
}

/** `base -- paths`, or a pathspec file when the list is long. */
async function withPaths(base: Cmd, paths: string[]): Promise<Cmd> {
  if (paths.length <= 25) return [...base, "--", ...paths];
  const file = await api.writeTempFile(`pathspec-${hash(repo())}-${base[1]}.txt`, paths.join("\0"));
  return [...base, `--pathspec-from-file=${file}`, "--pathspec-file-nul"];
}

const shortSha = (sha: string) => sha.slice(0, 7);

function validRefName(v: string) {
  if (!v.trim()) return "Name is required";
  if (/[\s~^:?*\[\\]|\.\.|@\{|^[-/]|[/.]$|\.lock$/.test(v)) return "Invalid git ref name";
  return null;
}

// ---------------------------------------------------------------------------------------------
// Remote
// ---------------------------------------------------------------------------------------------

export const fetchAll = () => run(["git", "fetch", "--all", "--prune"]);

export async function pull(mode: "merge" | "rebase" | "ff-only" = "merge") {
  const flag = mode === "rebase" ? "--rebase" : mode === "ff-only" ? "--ff-only" : "--no-rebase";
  const cmd = ["git", "pull", flag, ...(mode === "merge" ? ["--no-edit"] : [])];
  const choice = await askLocalChanges(`pulling (${mode})`, "Smart pull — stash, pull, unstash (recommended)", false);
  if (choice === null) return;
  if (choice !== "smart") return run(cmd);
  // If the pull stops (conflicts, rejected fast-forward) the stash stays; the commit panel offers to unstash later.
  run(["git", "stash", "push", "-m", `${SMART_STASH_PREFIX} pull: ${getState().info?.branch ?? "HEAD"}`], cmd, ["git", "stash", "pop"]);
}

/**
 * With uncommitted changes to tracked files, asks how to handle them.
 * Returns "none" (nothing to handle), "smart", "keep", "force" or null (cancelled).
 */
async function askLocalChanges(what: string, smartLabel: string, allowForce: boolean): Promise<string | null> {
  const dirty = getState().status.filter((f) => !f.untracked);
  if (!dirty.length) return "none";
  if (dirty.some((f) => f.conflicted)) {
    toast("Resolve the conflicted files first", "error");
    return null;
  }
  const v = await form({
    title: "Local changes",
    message: `You have ${dirty.length} uncommitted change${dirty.length === 1 ? "" : "s"}. How should they be handled when ${what}?`,
    fields: [
      {
        name: "mode",
        label: "Mode",
        type: "select",
        options: [
          { value: "smart", label: smartLabel },
          { value: "keep", label: "Keep changes — run as is (git refuses if they conflict)" },
          ...(allowForce ? [{ value: "force", label: "Force — DISCARD local changes" }] : []),
        ],
      },
    ],
    ok: "Continue",
  });
  return v ? v.mode : null;
}

export function pushCommand(force = false, branch?: string): Cmd {
  const info = getState().info;
  const name = branch ?? info?.branch;
  if (!info || !name) throw new Error("Not on a branch (detached HEAD) — create a branch first");
  const b = info.branches.find((x) => x.name === name);
  const f = force ? ["--force-with-lease"] : [];
  if (b?.upstream && !b.gone) {
    if (branch && !b.current) {
      const [remote, ...rest] = b.upstream.split("/");
      return ["git", "push", ...f, remote, `${name}:${rest.join("/")}`];
    }
    return ["git", "push", ...f];
  }
  const remote = info.remotes.includes("origin") ? "origin" : info.remotes[0];
  if (!remote) throw new Error("This repository has no remotes");
  return ["git", "push", ...f, "-u", remote, name];
}

export function push(force = false, branch?: string) {
  try {
    return run(pushCommand(force, branch));
  } catch (e) {
    toastError(e);
  }
}

export const pushTags = () => run(["git", "push", "--tags"]);

// ---------------------------------------------------------------------------------------------
// Branches & tags
// ---------------------------------------------------------------------------------------------

// Smart checkout (JetBrains style): with uncommitted changes, stash them, switch, unstash.

/** Runs `git switch <args>`; asks how to handle uncommitted tracked changes first. */
async function smartSwitch(switchArgs: string[], target: string) {
  const mode = await askLocalChanges(`checking out ${target}`, "Smart checkout — stash, checkout, unstash (recommended)", true);
  if (mode === null) return;
  if (mode === "none" || mode === "keep") return run(["git", "switch", ...switchArgs]);
  if (mode === "force") {
    if (await confirm("Force checkout", "All uncommitted changes to tracked files will be lost. Continue?", "Discard and checkout", true))
      run(["git", "switch", "--discard-changes", ...switchArgs]);
    return;
  }
  const from = getState().info?.branch ?? shortSha(getState().info?.headSha ?? "HEAD");
  // If `git switch` fails the chain stops and the stash stays; the commit panel then offers to unstash.
  run(
    ["git", "stash", "push", "-m", `${SMART_STASH_PREFIX} checkout: ${from} -> ${target}`],
    ["git", "switch", ...switchArgs],
    ["git", "stash", "pop"],
  );
}

export function checkoutBranch(name: string) {
  return smartSwitch([name], name);
}

export function checkoutRemote(remoteRef: string) {
  const local = remoteRef.split("/").slice(1).join("/");
  const info = getState().info;
  if (info?.branches.some((b) => b.name === local)) return smartSwitch([local], local);
  return smartSwitch(["--track", remoteRef], local);
}

export async function checkoutCommit(sha: string) {
  const ok = await confirm(
    "Checkout commit",
    `Check out ${shortSha(sha)} in detached HEAD state? Commits made there will not belong to any branch.`,
    "Checkout",
  );
  if (ok) smartSwitch(["--detach", sha], shortSha(sha));
}

/** The newest stash, if it was created by a smart checkout. */
export function smartStash() {
  const st = getState().info?.stashes[0];
  return st && st.message.includes(SMART_STASH_PREFIX) ? st : null;
}

export async function createBranch(at?: string, label?: string) {
  const v = await form({
    title: "Create branch",
    message: at ? `Start point: ${label ?? shortSha(at)}` : "Start point: HEAD",
    fields: [
      { name: "name", label: "Branch name", type: "text", placeholder: "feature/my-change" },
      { name: "checkout", label: "Checkout the new branch", type: "checkbox", value: true },
    ],
    ok: "Create",
    validate: (v) => validRefName(v.name),
  });
  if (!v) return;
  const name = v.name.trim();
  if (v.checkout && at) smartSwitch(["-c", name, at], name);
  else if (v.checkout) run(["git", "switch", "-c", name]);
  else run(["git", "branch", name, ...(at ? [at] : [])]);
}

export async function renameBranch(name: string) {
  const v = await form({
    title: "Rename branch",
    fields: [{ name: "name", label: "New name", type: "text", value: name }],
    ok: "Rename",
    validate: (v) => validRefName(v.name),
  });
  if (v && v.name.trim() !== name) run(["git", "branch", "-m", name, v.name.trim()]);
}

export async function deleteBranch(name: string) {
  const b = getState().info?.branches.find((x) => x.name === name);
  const fields: any[] = [{ name: "force", label: "Force delete (even if not merged)", type: "checkbox", value: false }];
  if (b?.upstream && !b.gone) fields.push({ name: "remote", label: `Also delete remote branch ${b.upstream}`, type: "checkbox", value: false });
  const v = await form({ title: "Delete branch", message: `Delete local branch '${name}'?`, fields, ok: "Delete", danger: true });
  if (!v) return;
  const cmds: Cmd[] = [["git", "branch", v.force ? "-D" : "-d", name]];
  if (v.remote && b?.upstream) {
    const [remote, ...rest] = b.upstream.split("/");
    cmds.push(["git", "push", remote, "--delete", rest.join("/")]);
  }
  run(...cmds);
}

export async function deleteRemoteBranch(remoteRef: string) {
  const [remote, ...rest] = remoteRef.split("/");
  if (await confirm("Delete remote branch", `Delete '${rest.join("/")}' on remote '${remote}'?`, "Delete", true))
    run(["git", "push", remote, "--delete", rest.join("/")]);
}

export async function setUpstream(name: string) {
  const info = getState().info;
  if (!info) return;
  const v = await form({
    title: "Set upstream",
    message: `Upstream branch for '${name}'`,
    fields: [
      {
        name: "upstream",
        label: "Remote branch",
        type: "select",
        options: info.remoteBranches.map((r) => ({ value: r.name, label: r.name })),
        value: info.remoteBranches.find((r) => r.name.endsWith("/" + name))?.name,
      },
    ],
    ok: "Set",
  });
  if (v?.upstream) run(["git", "branch", `--set-upstream-to=${v.upstream}`, name]);
}

export async function createTag(sha: string) {
  const info = getState().info;
  const v = await form({
    title: "Create tag",
    message: `Tag commit ${shortSha(sha)}`,
    fields: [
      { name: "name", label: "Tag name", type: "text", placeholder: "v1.0.0" },
      { name: "message", label: "Message (makes an annotated tag)", type: "textarea" },
      ...(info?.remotes.length ? [{ name: "push", label: "Push tag to remote", type: "checkbox" as const, value: false }] : []),
    ],
    ok: "Create",
    validate: (v) => validRefName(v.name),
  });
  if (!v) return;
  const name = v.name.trim();
  const cmds: Cmd[] = [];
  if (v.message.trim()) {
    const file = await api.writeTempFile(`TAG_MSG_${hash(repo())}.txt`, v.message);
    cmds.push(["git", "tag", "-a", name, "-F", file, sha]);
  } else cmds.push(["git", "tag", name, sha]);
  if (v.push) cmds.push(["git", "push", info!.remotes.includes("origin") ? "origin" : info!.remotes[0], `refs/tags/${name}`]);
  run(...cmds);
}

export async function deleteTag(name: string) {
  const info = getState().info;
  const fields: any[] = info?.remotes.length
    ? [{ name: "remote", label: "Also delete from remote", type: "checkbox", value: false }]
    : [];
  const v = await form({ title: "Delete tag", message: `Delete tag '${name}'?`, fields, ok: "Delete", danger: true });
  if (!v) return;
  const cmds: Cmd[] = [["git", "tag", "-d", name]];
  if (v.remote) cmds.push(["git", "push", info!.remotes.includes("origin") ? "origin" : info!.remotes[0], "--delete", `refs/tags/${name}`]);
  run(...cmds);
}

export function pushTag(name: string) {
  const info = getState().info;
  if (!info?.remotes.length) return toast("No remotes configured", "error");
  run(["git", "push", info.remotes.includes("origin") ? "origin" : info.remotes[0], `refs/tags/${name}`]);
}

// ---------------------------------------------------------------------------------------------
// History operations
// ---------------------------------------------------------------------------------------------

function target() {
  return getState().info?.branch ?? "HEAD";
}

export async function merge(ref: string, label?: string) {
  const v = await form({
    title: "Merge",
    message: `Merge ${label ?? ref} into ${target()}`,
    fields: [
      {
        name: "mode",
        label: "Strategy",
        type: "select",
        options: [
          { value: "default", label: "Default (fast-forward if possible)" },
          { value: "no-ff", label: "Always create a merge commit (--no-ff)" },
          { value: "ff-only", label: "Fast-forward only (--ff-only)" },
          { value: "squash", label: "Squash (--squash, commit manually)" },
        ],
      },
    ],
    ok: "Merge",
  });
  if (!v) return;
  const flags = v.mode === "default" ? [] : [`--${v.mode}`];
  run(["git", "merge", ...(v.mode === "squash" ? [] : ["--no-edit"]), ...flags, ref]);
}

export async function rebase(ref: string, label?: string) {
  if (await confirm("Rebase", `Rebase ${target()} onto ${label ?? ref}?`, "Rebase"))
    run(["git", "rebase", ref]);
}

export async function cherryPick(row: GraphRow) {
  const isMerge = row.parents.length > 1;
  if (await confirm("Cherry-pick", `Apply ${shortSha(row.sha)} "${row.subject}" on top of ${target()}?`, "Cherry-pick"))
    run(["git", "cherry-pick", ...(isMerge ? ["-m", "1"] : []), row.sha]);
}

export async function revert(row: GraphRow) {
  const isMerge = row.parents.length > 1;
  if (await confirm("Revert commit", `Create a new commit that reverts ${shortSha(row.sha)} "${row.subject}"?`, "Revert"))
    run(["git", "revert", "--no-edit", ...(isMerge ? ["-m", "1"] : []), row.sha]);
}

export async function reset(sha: string, mode?: "soft" | "mixed" | "hard") {
  const v = await form({
    title: "Reset current branch",
    message: `Move ${target()} to ${shortSha(sha)}`,
    fields: [
      {
        name: "mode",
        label: "Mode",
        type: "select",
        value: mode ?? "mixed",
        options: [
          { value: "soft", label: "Soft — keep changes staged" },
          { value: "mixed", label: "Mixed — keep changes in working tree" },
          { value: "hard", label: "Hard — DISCARD all local changes" },
        ],
      },
    ],
    ok: "Reset",
    danger: mode === "hard",
  });
  if (!v) return;
  if (v.mode === "hard" && !(await confirm("Hard reset", "All uncommitted changes will be lost. Continue?", "Reset --hard", true))) return;
  run(["git", "reset", `--${v.mode}`, sha]);
}

export async function editHeadMessage() {
  setState({ selected: "WIP", amendRequest: getState().amendRequest + 1 });
}

export async function squashInto(row: GraphRow) {
  // Soft reset to the parent of the chosen commit and recommit everything after it.
  if (!row.parents[0]) return toast("Cannot squash the root commit", "error");
  const ok = await confirm(
    "Squash commits",
    `Squash all commits from ${shortSha(row.sha)} up to HEAD into one? (git reset --soft ${shortSha(row.parents[0])}, then commit from the commit panel)`,
    "Squash",
  );
  if (!ok) return;
  await run(["git", "reset", "--soft", row.parents[0]]);
  setState({ selected: "WIP", focusCommitRequest: getState().focusCommitRequest + 1 });
}

// ---------------------------------------------------------------------------------------------
// Stash
// ---------------------------------------------------------------------------------------------

export async function stashPush() {
  const v = await form({
    title: "Stash changes",
    fields: [
      { name: "message", label: "Message (optional)", type: "text" },
      { name: "untracked", label: "Include untracked files", type: "checkbox", value: true },
      { name: "keepIndex", label: "Keep staged changes in the index", type: "checkbox", value: false },
    ],
    ok: "Stash",
  });
  if (!v) return;
  run([
    "git",
    "stash",
    "push",
    ...(v.untracked ? ["-u"] : []),
    ...(v.keepIndex ? ["--keep-index"] : []),
    ...(v.message.trim() ? ["-m", v.message.trim()] : []),
  ]);
}

export const stashApply = (name: string) => run(["git", "stash", "apply", name]);
export const stashPop = (name?: string) => run(["git", "stash", "pop", ...(name ? [name] : [])]);
export async function stashDrop(name: string) {
  if (await confirm("Drop stash", `Drop ${name}? This cannot be undone.`, "Drop", true)) run(["git", "stash", "drop", name]);
}

// ---------------------------------------------------------------------------------------------
// Working tree / commit
// ---------------------------------------------------------------------------------------------

const pathsOf = (f: FileChange) => (f.origPath ? [f.path, f.origPath] : [f.path]);

export async function commit(opts: { files: FileChange[]; checked: Set<string>; message: string; amend: boolean; push: boolean }) {
  const r = repo();
  const info = getState().info;
  const cmds: Cmd[] = [];
  const unstage = opts.files.filter((f) => f.staged && !opts.checked.has(f.path)).flatMap(pathsOf);
  const add = opts.files.filter((f) => opts.checked.has(f.path)).flatMap(pathsOf);
  if (unstage.length) cmds.push(await withPaths(["git", "reset", "-q"], unstage));
  if (add.length) cmds.push(await withPaths(["git", "add", "-A"], add));

  if (info?.state === "rebase") {
    cmds.push(["git", "-c", "core.editor=true", "rebase", "--continue"]);
  } else {
    if (!opts.message.trim()) return toast("Commit message is empty", "error");
    const file = await api.writeTempFile(`COMMIT_MSG_${hash(r)}.txt`, opts.message.trim() + "\n");
    cmds.push(["git", "commit", ...(opts.amend ? ["--amend"] : []), "-F", file]);
  }
  if (opts.push) {
    try {
      cmds.push(pushCommand(opts.amend));
    } catch (e) {
      return toastError(e);
    }
  }
  await run(...cmds);
  return true;
}

export async function stageFiles(files: FileChange[]) {
  run(await withPaths(["git", "add", "-A"], files.flatMap(pathsOf)));
}

export async function unstageFiles(files: FileChange[]) {
  run(await withPaths(["git", "reset", "-q"], files.flatMap(pathsOf)));
}

export async function rollback(files: FileChange[]) {
  if (!files.length) return;
  const names = files.length === 1 ? files[0].path : `${files.length} files`;
  if (!(await confirm("Rollback changes", `Discard all local changes in ${names}? Untracked files will be deleted.`, "Rollback", true))) return;
  const tracked = files.filter((f) => !f.untracked && f.x !== "A");
  const added = files.filter((f) => f.x === "A");
  const untracked = files.filter((f) => f.untracked);
  const cmds: Cmd[] = [];
  const hasHead = !!getState().info?.headSha;
  if (added.length) cmds.push(await withPaths(["git", "rm", "--cached", "-q", "-f"], added.map((f) => f.path)));
  if (tracked.length && hasHead) cmds.push(await withPaths(["git", "restore", "--source=HEAD", "--staged", "--worktree"], tracked.flatMap(pathsOf)));
  if (untracked.length || added.length) cmds.push(await withPaths(["git", "clean", "-f", "-q"], [...untracked, ...added].map((f) => f.path)));
  run(...cmds);
}

export async function acceptSide(f: FileChange, side: "ours" | "theirs") {
  const xy = f.x + f.y;
  const deleted = side === "ours" ? xy === "DU" || xy === "DD" : xy === "UD" || xy === "DD";
  if (deleted) run(["git", "rm", "-q", "--", f.path]);
  else run(["git", "checkout", `--${side}`, "--", f.path], ["git", "add", "--", f.path]);
}

export const markResolved = (path: string) => run(["git", "add", "--", path]);

export function abortOperation() {
  const st = getState().info?.state;
  if (!st || st === "none") return;
  confirm("Abort", `Abort the ${st} and return to the state before it started?`, "Abort", true).then(
    (ok) => void (ok && run(["git", st, "--abort"])),
  );
}

export function continueOperation() {
  const st = getState().info?.state;
  if (!st || st === "none") return;
  if (st === "merge") return run(["git", "commit", "--no-edit"]);
  run(["git", "-c", "core.editor=true", st, "--continue"]);
}

export function skipOperation() {
  const st = getState().info?.state;
  if (st === "rebase" || st === "cherry-pick" || st === "revert") run(["git", st, "--skip"]);
}

export function initRepo() {
  return run(["git", "init"]).then(() => setTimeout(() => refreshAll().catch(() => {}), 800));
}

export function copy(text: string) {
  navigator.clipboard.writeText(text).then(() => toast("Copied to clipboard")).catch(toastError);
}

export { currentBranch };
