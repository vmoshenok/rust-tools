# Git Ruster — git client with graph, built-in terminal, diff viewer and 3-way merge

A desktop git client (Tauri 2: Rust backend + web UI) in the spirit of GitKraken, with a
different UX:

* **Left**: a list of folders / repositories.
* **Center**: the commit graph of the selected repository, with details (or the commit panel) on the right.
* **Bottom**: a real built-in terminal. **Every operation that changes a repository is typed
  into this terminal as a plain git command**, so you always see exactly what runs,
  can answer prompts (credentials, SSH passphrases), and can repeat or tweak it by hand.

Reading (log, status, diffs, file contents) is done in the background with the git CLI;
writing (commit, merge, rebase, push, …) is always visible in the terminal.

## Layout

```
┌──────────────┬──────────────────────────────────────────────┬────────────────────┐
│ REPOSITORIES │ repo · branch ▾  Fetch Pull▾ Push▾ │ Branch Stash Pop │ Commit │ ⌨ ⟳ ↗ │
│  repo-a      ├──────────────────────────────────────────────┼────────────────────┤
│  workspace/  │ Graph │ refs + message        │ author │ date │ hash │ Commit details │
│    repo-b    │  ◌    // WIP  3 changed files                  │   or            │
│    repo-c    │  ●    [main☁] Fix parser                       │ Commit panel    │
│──────────────│  │●   [feature] Add tests                      │ (checkboxes,    │
│ LOCAL / REMOTES / TAGS / STASHES                              │  message, Amend,│
│ (selected repo)                                               │  Commit & Push) │
├──────────────┴──────────────────────────────────────────────┴────────────────────┤
│ Terminal (one shell per repository, cwd = repo root)                              │
└───────────────────────────────────────────────────────────────────────────────────┘
```

All panels are resizable (sizes persist). The diff viewer and the merge tool open full-window (Esc closes).

## Features

### Repositories (left)
* **Add folder** (`+`), drag & drop folders onto the window, or Explorer → *Open in Git Ruster*.
* A folder that is a repository is shown as a repo. A folder that **contains** repositories
  (scanned up to 3 levels deep, skipping `node_modules`, `target`, `dist`, … and hidden dirs)
  becomes a group with its repos as children. A folder without repos can be initialized (`git init`).
* Opening a sub-folder of a repository adds the repository root.
* Context menu: Open in Explorer, Copy path, Rescan, Remove from list.
* Below the list, for the selected repo: **local branches** (with ↑ahead/↓behind, `gone`),
  **remotes**, **tags**, **stashes**. Click = jump to the commit in the graph, double-click =
  checkout (local / tracking) or apply (stash), right-click = actions.

### Commit graph (center)
* All branches, remotes, tags and HEAD (`git log --branches --remotes --tags HEAD --date-order`),
  up to *Max commits* (settings, default 3000). Virtualized, so big histories scroll smoothly.
* Lanes with stable colors, merge curves, HEAD node ring, merge commits as small dots.
* A `// WIP` row on top when there are uncommitted changes (or a merge/rebase is in progress).
* Ref badges: local branch (✓ = current), remote (dashed), tag (gold), detached HEAD.
  `main` and `origin/main` on the same commit merge into one badge with a cloud marker.
* Search box: message, hash prefix, author, ref name. Enter / Shift+Enter = next / previous match.
* Keyboard: ↑/↓ moves the selection.

**Branch filter & lane priority** (per repository, toolbar filter button — highlighted when
active; also *Filter / sort branches…*, *Hide in graph*, *Graph priority…* in branch menus):
* Checkbox per local and remote branch: which branches the graph shows. New branches are
  visible by default (the setting stores the hidden ones). The current branch / HEAD is always shown.
* **Only local** — only local branches plus their upstream (remote-tracking) branches.
* **Show tags** — include commits reachable only from tags.
* **Priority** column — empty = default layout. Priority 1 draws that branch in the leftmost lane,
  2 in the next lane, and so on, each with a fixed color. A prioritized branch owns its
  first-parent history (a higher priority wins shared history, e.g. develop=1 keeps the common
  history with master in lane 0). Other branches are laid out to the right of the reserved lanes.
* With a filter active the graph runs `git log` on just the visible refs (+ HEAD), and only
  their badges are shown. Hidden branches are dimmed in the sidebar; prioritized ones show `#n`.
* Stored in `config.json` under `repos["<repo path>"]` (`hidden`, `onlyLocal`, `hideTags`, `priorities`).

**Comparing revisions**:
* **Two commits**: click one commit, **Ctrl+click** another (marked with a blue bar) → right-click
  either → *Show diff A ↔ B*. The older commit is on the left. Ctrl+click again unmarks.
* **Show diff with working tree** (commit, branch, remote branch, tag menus) — that revision vs the
  current files on disk, including untracked files; » revert buttons work on the right side.
* **Show diff with…** — a picker with all branches, remote branches, tags and the loaded commits;
  type to filter by name, hash, message or author (several words = all must match), ↑/↓ + Enter.
  Any git revision expression can be typed directly (`HEAD~3`, `v1.0^`, a sha prefix).
* Files come from `git diff --name-status -M <a> [<b>]`; the diff viewer has a **Swap sides** button.

**Smart checkout** (JetBrains style): checking out a branch / remote branch / commit / tag, or
creating a branch from another start point, while tracked files have uncommitted changes asks:
* *Smart checkout* (default): `git stash push -m "git-ruster smart checkout: A -> B"` →
  `git switch B` → `git stash pop`.
* *Keep changes*: plain `git switch` (git refuses if the changes conflict).
* *Force checkout*: `git switch --discard-changes` (after a confirmation).

**Smart pull** works the same way for every Pull (toolbar, Pull ▾ variants, branch menu):
*Smart pull* (default) runs `git stash push -m "git-ruster smart pull: <branch>"` → `git pull …` →
`git stash pop`; *Keep changes* runs the plain pull. If the pull itself stops (merge/rebase
conflicts in the pulled commits, rejected fast-forward), finish or abort it first — the stash
stays and the banner then offers **Unstash (pop)**.

If unstashing conflicts, the commit panel shows a *Smart checkout* / *Smart pull* banner: resolve the files in the
merge tool (left = *Checked out (branch)* / *Pulled (branch)*, right = *Your stashed changes*), then **Drop stash** (git
keeps the stash after a conflicting pop). If the switch itself failed, the banner offers
**Unstash (pop)** to restore the changes. Untracked files are not stashed.

**Commit context menu**: Checkout branch(es) on it, Checkout commit (detached), Create branch here,
Create tag here, Show diff with working tree / with…, Merge into current, Rebase current onto, Cherry-pick, Revert, Reset current
branch to here (soft / mixed / hard), Amend (HEAD), Undo commit (HEAD, soft reset), Squash
`X..HEAD` into one commit (commits on HEAD's first-parent chain), Copy hash / subject.

**Branch badge menu**: Checkout, Merge, Rebase, Push, New branch from here, Rename, Set upstream,
Delete (optionally also on the remote). **Remote badge**: Checkout (track), Merge, Rebase,
New branch, Delete remote branch. **Tag**: Checkout, Merge, New branch, Push, Delete (optionally remote).

### Toolbar
Branch switcher, Fetch, Pull ▾ (merge / rebase / ff-only), Push ▾ (push, force-with-lease, push tags),
Branch, Stash, Pop, Commit (Ctrl+K), Terminal (Ctrl+`), Refresh (F5), Open in Explorer.
Pull/Push show behind/ahead counts.

### Commit details (right, when a commit or stash is selected)
Subject, body, refs, author/committer with dates, hash (click = copy), parents (click = go to),
changed files (status letter, rename info). Double-click / Enter on a file opens the diff viewer
(with all files of the commit, navigable). Stashes additionally get Apply / Pop / Drop.

### Commit panel (right, when `// WIP` is selected) — JetBrains style
* Groups: **Merge Conflicts**, **Changes**, **Unversioned Files**, each with a tri-state checkbox.
* A checkbox selects what goes into the commit (like a JetBrains changelist), regardless of the
  current index. Defaults: tracked changes checked, unversioned unchecked, files that were just
  resolved in a merge become checked. A green dot marks files that are staged in the index.
* Commit message (draft kept per repository), first-line length counter, **Amend** (loads the
  last message), **Commit** (Ctrl+Enter) and **Commit and Push** (Ctrl+Shift+Enter).
* File menu: Show diff, Stage, Unstage, Rollback (discard, deletes unversioned), Copy path,
  Show in Explorer; for conflicts: Resolve…, Accept Yours, Accept Theirs, Mark as resolved.
* While a merge / rebase / cherry-pick / revert is in progress a banner shows the operation
  with **Resolve conflicts…**, **Continue**, **Skip**, **Abort**. The commit button becomes
  *Commit Merge* or *Continue Rebase*; for merge / cherry-pick / revert the prepared message
  (`MERGE_MSG`) is filled in.

### Diff viewer
* Side-by-side (aligned, filler hatching for missing lines) or unified.
* JetBrains colors: modified = blue, added = green, deleted = gray; **word-level** highlighting
  inside modified blocks; syntax highlighting (highlight.js, ~40 languages by extension).
* *Collapse unchanged* fragments (click to expand), *Ignore whitespace*.
* Navigate changes with F7 / Shift+F7 (or Alt+↓/↑; at the end it moves on to the next file),
  files with Alt+←/→ or the file list on the left; overview ruler with change marks on the right.
* For working-tree diffs: **»** reverts a single change block in the file.
* Long lines: Shift+wheel (or horizontal wheel) scrolls both sides together.

### 3-way merge tool
* Columns: **Yours/Left** │ **Result** │ **Theirs/Right**, aligned per region, with real labels
  (merge: `Yours (main)` / `Theirs (feature)`; rebase: `Upstream (onto abc1234)` / `Your commit …`).
* Content comes from the index stages (`:1:` base, `:2:` ours, `:3:` theirs); the region split is
  a diff3 merge computed against the base.
* Non-conflicting changes (one side only, or identical on both sides) are applied automatically
  and shown in blue; conflicts are red.
* Per change: **»** / **«** accept a side, **×** ignore it, **+** append the other side as well
  (both, in order), **↺** undo. The result of every change block is an editable text area.
* Toolbar: conflict counter, Prev/Next (F7), Apply non-conflicting, Accept Left (all), Accept Right
  (all), Cancel, **Apply** — writes the file (keeping its CRLF/LF style) and runs `git add` in
  the terminal.
* Modify/delete and binary conflicts offer "use ours / use theirs".

### Built-in terminal
* xterm.js on a real ConPTY (portable-pty), one shell per repository, started in the repo root.
* Shell: Auto (PowerShell 7 → Windows PowerShell), pwsh, powershell, cmd or Git Bash (settings).
* Commands from the UI are quoted for the selected shell and chained so that each step runs only
  if the previous one succeeded (`&&`, or `; if ($?) { … }` on Windows PowerShell 5.1).
  Commit messages are passed via a temp file (`git commit -F`), long path lists via
  `--pathspec-from-file`.
* Ctrl+C copies when text is selected (otherwise interrupts), Ctrl+V pastes. Clear / restart shell / hide.
  If the shell exits, press Enter to start a new one.

### Auto refresh
The selected repository is watched (`notify`): changes inside `.git` (refs, HEAD, index — i.e.
any git command, also ones you type yourself) reload the graph; working-tree changes reload the
status. Also refreshed on window focus and with F5.

### Windows Explorer integration
Settings → *Add "Open in Git Ruster" to folder menu* registers (per user, no admin):

```
HKCU\Software\Classes\Directory\shell\GitRuster\command             "<exe>" "%1"
HKCU\Software\Classes\Directory\Background\shell\GitRuster\command  "<exe>" "%V"
```

Right-click a folder (or the empty space inside one) → *Open in Git Ruster*. The app is
single-instance: a second launch forwards the folder to the running window, which adds it to
the list (or selects it if already listed) and comes to the front. On Windows 11 the item is
under *Show more options*. Re-register after moving the executable.

## Git commands used

| UI action | Command (typed into the terminal) |
|---|---|
| Fetch | `git fetch --all --prune` |
| Pull | `git pull --no-rebase --no-edit` / `--rebase` / `--ff-only` |
| Push | `git push` or `git push -u <origin> <branch>` (no upstream); `--force-with-lease` |
| Push other branch | `git push <remote> <branch>:<upstream-branch>` |
| Checkout | `git switch <branch>` · `git switch --track origin/x` · `git switch --detach <sha>` |
| Create branch | `git switch -c <name> [<start>]` or `git branch <name> [<start>]` |
| Rename / delete branch | `git branch -m old new` · `git branch -d/-D name` · `git push <remote> --delete name` |
| Set upstream | `git branch --set-upstream-to=<remote/branch> <name>` |
| Tag | `git tag <name> <sha>` · `git tag -a <name> -F <msgfile> <sha>` · `git push <remote> refs/tags/<name>` · `git tag -d` |
| Merge | `git merge --no-edit [--no-ff/--ff-only] <ref>` · `git merge --squash <ref>` |
| Rebase | `git rebase <ref>` |
| Cherry-pick / Revert | `git cherry-pick [-m 1] <sha>` · `git revert --no-edit [-m 1] <sha>` |
| Reset | `git reset --soft/--mixed/--hard <sha>` |
| Commit | `git reset -q -- <unchecked staged>` → `git add -A -- <checked>` → `git commit [--amend] -F <msgfile>` [→ push] |
| Rollback | `git restore --source=HEAD --staged --worktree -- …` · `git rm --cached` + `git clean -f -- …` |
| Stage / unstage | `git add -A -- …` · `git reset -q -- …` |
| Stash | `git stash push [-u] [--keep-index] [-m msg]` · `apply` / `pop` / `drop stash@{n}` |
| Conflicts | `git checkout --ours/--theirs -- f` + `git add -- f` · `git rm -- f` · `git add -- f` (resolved) |
| Continue / skip / abort | `git commit --no-edit` (merge) · `git -c core.editor=true rebase/cherry-pick/revert --continue` · `--skip` · `--abort` |
| Squash X..HEAD | `git reset --soft <X^>` then commit from the panel |
| Init | `git init` |

## Keyboard shortcuts

| Key | Action |
|---|---|
| Ctrl+K | Commit panel (focus message) |
| Ctrl+Enter / Ctrl+Shift+Enter | Commit / Commit and Push |
| Ctrl+` | Toggle terminal |
| F5 | Refresh |
| ↑ / ↓ (graph focused) | Select previous / next commit |
| F7 / Shift+F7 | Next / previous change (diff viewer, merge tool) |
| Alt+← / Alt+→ | Previous / next file (diff viewer) |
| Esc | Close diff viewer / merge tool / dialog / menu |

## Architecture

```
git-ruster/
├─ index.html, vite.config.ts, tsconfig.json, package.json
├─ src/                        UI (Preact + TypeScript, no UI framework)
│  ├─ api.ts                   typed wrappers for the Rust commands
│  ├─ state.ts                 app store, loading/refresh logic, watcher events
│  ├─ actions.ts               all git operations → commands for the terminal
│  ├─ terminal.ts              xterm.js instances per repo, PTY event routing
│  ├─ ui.tsx                   context menus, form dialogs, toasts (imperative API)
│  ├─ lib/diff.ts              line diff, word diff, diff3 regions, highlight.js → tokens
│  ├─ lib/css.ts               px() helper — Preact 11 does not append "px" to numeric
│  │                           inline styles, so lengths must carry a unit (`top: px(n)`)
│  ├─ styles.css               dark theme (JetBrains-like)
│  └─ components/              App, Sidebar, Toolbar, Graph, Details (commit details +
│                              commit panel), TerminalPanel, DiffViewer, MergeTool,
│                              Settings, menus, Splitter, Icon, common
└─ src-tauri/                  Rust backend
   ├─ tauri.conf.json, capabilities/default.json, icons/
   └─ src/
      ├─ main.rs               setup, plugins (single-instance, dialog), command list
      ├─ config.rs             config.json, folder list/scan, CLI argument → open folder
      ├─ git.rs                read-only git queries (info, refs, status, details, blobs)
      ├─ graph.rs              lane layout for the commit graph
      ├─ pty.rs                ConPTY sessions, term_run (types commands + Enter)
      ├─ shell.rs              shell detection, per-shell quoting and && chaining
      ├─ watcher.rs            file-system watcher → `repo-changed` events
      ├─ explorer.rs           Explorer context menu (HKCU registry)
      └─ util.rs               path helpers, hidden child processes
```

Configuration is stored in `%APPDATA%\com.vmoshenok.gitruster\config.json`
(folders, selected repo, settings, panel sizes). Temporary message/pathspec files go to
`%TEMP%\git-ruster\`.

## Build & run

Prerequisites: Rust (MSVC toolchain + Visual Studio Build Tools with the C++ workload and a
Windows SDK), Node.js 18+, git on `PATH`, WebView2 (built into Windows 10/11).

```powershell
npm install
npm run tauri dev      # development (hot reload for the UI)
npm run tauri build    # release: src-tauri\target\release\git-ruster.exe + NSIS installer
```

Usage: `git-ruster.exe [folder]` — opens/adds the folder.

## Not yet implemented / ideas
* Interactive rebase editor (reorder / squash / reword in the graph), drag & drop branch onto branch.
* Line/hunk-level partial commits (currently whole files).
* File history / blame, compare two arbitrary commits, submodules, worktrees, LFS.
* Light theme.
