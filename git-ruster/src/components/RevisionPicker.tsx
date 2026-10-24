import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { RevisionPickerRequest, setState, useAppState } from "../state";
import { Icon } from "./Icon";
import { Avatar, formatDate } from "./common";

interface Entry {
  kind: "branch" | "remote" | "tag" | "commit" | "typed";
  rev: string;
  label: string;
  /** commit subject / extra info */
  detail?: string;
  author?: string;
  time?: number;
  search: string;
}

const MAX_SHOWN = 300;

/** "Show diff with…" picker: branches, tags and loaded commits, searchable. */
export function RevisionPicker({ req }: { req: RevisionPickerRequest }) {
  const s = useAppState();
  const info = s.info;
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const entries = useMemo<Entry[]>(() => {
    const out: Entry[] = [];
    info?.branches.forEach((b) =>
      out.push({ kind: "branch", rev: b.name, label: b.name, detail: b.current ? "current branch" : undefined, search: b.name.toLowerCase() }),
    );
    info?.remoteBranches.forEach((b) => out.push({ kind: "remote", rev: b.name, label: b.name, search: b.name.toLowerCase() }));
    [...(info?.tags ?? [])].reverse().forEach((t) => out.push({ kind: "tag", rev: t.name, label: t.name, search: t.name.toLowerCase() }));
    s.graph?.rows.forEach((r) => {
      if (r.wip) return;
      out.push({
        kind: "commit",
        rev: r.sha,
        label: r.sha.slice(0, 7),
        detail: r.subject,
        author: r.author,
        time: r.time,
        search: `${r.sha} ${r.subject} ${r.author} ${r.email}`.toLowerCase(),
      });
    });
    return out;
  }, [info, s.graph]);

  const filtered = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const list = words.length ? entries.filter((e) => words.every((w) => e.search.includes(w))) : entries;
    const shown = list.slice(0, MAX_SHOWN);
    // Allow any revision expression git understands (sha prefix, HEAD~3, v1.0^, ...).
    const q = query.trim();
    if (q && !/\s/.test(q) && !shown.some((e) => e.rev === q))
      shown.push({ kind: "typed", rev: q, label: q, detail: "use as typed (any git revision)", search: "" });
    return shown;
  }, [entries, query]);

  useEffect(() => setActive(0), [query]);
  useEffect(() => {
    listRef.current?.querySelector(".rp-row.active")?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const finish = (e: Entry | null) => {
    setState({ picker: null });
    req.resolve(e ? { rev: e.rev, label: e.kind === "commit" ? e.rev.slice(0, 7) : e.label } : null);
  };

  const icon = (k: Entry["kind"]) => (k === "tag" ? "tag" : k === "remote" ? "cloud" : k === "branch" ? "branch" : k === "typed" ? "right" : "commit");

  return (
    <div class="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && finish(null)}>
      <div class="dialog wide rp-dialog">
        <div class="dialog-title">{req.title}</div>
        <span class="graph-search rp-search">
          <Icon name="search" size={14} />
          <input
            type="text"
            autoFocus
            ref={(el) => void (el && setTimeout(() => el.focus(), 0))}
            placeholder="Branch, tag, commit hash, message or author…"
            value={query}
            spellcheck={false}
            onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive((a) => Math.min(filtered.length - 1, a + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => Math.max(0, a - 1));
              } else if (e.key === "Enter" && filtered[active]) finish(filtered[active]);
              else if (e.key === "Escape") finish(null);
            }}
          />
        </span>
        <div class="rp-list" ref={listRef}>
          {filtered.map((e, i) => (
            <div
              key={e.kind + e.rev}
              class={`rp-row ${i === active ? "active" : ""}`}
              onMouseEnter={() => setActive(i)}
              onClick={() => finish(e)}
            >
              <Icon name={icon(e.kind)} size={13} />
              <span class={e.kind === "commit" ? "mono rp-sha" : "rp-name"}>{e.label}</span>
              {e.detail && <span class="rp-detail">{e.detail}</span>}
              {e.author && (
                <span class="rp-meta">
                  <Avatar name={e.author} size={16} /> {e.author} · {formatDate(e.time ?? 0)}
                </span>
              )}
            </div>
          ))}
          {filtered.length === 0 && <div class="empty small">Nothing found</div>}
        </div>
        <div class="dialog-buttons">
          <span class="muted small bf-count">↑↓ to move, Enter to choose</span>
          <button onClick={() => finish(null)}>Cancel</button>
        </div>
      </div>
    </div>
  );
}
