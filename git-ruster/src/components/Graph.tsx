import { px } from "../lib/css";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { GraphRow, RefLabel } from "../api";
import * as A from "../actions";
import { getState, setState, useAppState } from "../state";
import { showMenu } from "../ui";
import { Icon } from "./Icon";
import { branchMenu, commitMenu, remoteMenu, tagMenu } from "./menus";
import { Avatar, formatDate } from "./common";

export const ROW_H = 28;
const LANE_W = 16;
const PAD = 8;

const COLORS = ["#4a9df8", "#f5a524", "#5fc35a", "#e8578b", "#a37cf2", "#2fc7d4", "#ff7a45", "#c3cf3a", "#e45be0", "#46c890", "#f0c541", "#7d8cff"];
export const laneColor = (i: number) => COLORS[i % COLORS.length];

const lx = (c: number) => PAD + c * LANE_W + LANE_W / 2;
const ly = (v: number) => (v === 0 ? 0 : v === 1 ? ROW_H / 2 : ROW_H);

function GraphCell({ row, width }: { row: GraphRow; width: number }) {
  const cx = lx(row.col);
  const cy = ROW_H / 2;
  const color = laneColor(row.color);
  return (
    <svg class="graph-svg" width={width} height={ROW_H}>
      {row.segs.map(([x1, y1, x2, y2, c], i) => {
        const X1 = lx(x1), Y1 = ly(y1), X2 = lx(x2), Y2 = ly(y2);
        const stroke = laneColor(c);
        if (X1 === X2) return <line key={i} x1={X1} y1={Y1} x2={X2} y2={Y2} stroke={stroke} stroke-width="2" />;
        const ym = (Y1 + Y2) / 2;
        return <path key={i} d={`M${X1} ${Y1} C${X1} ${ym} ${X2} ${ym} ${X2} ${Y2}`} stroke={stroke} stroke-width="2" fill="none" />;
      })}
      {row.wip ? (
        <circle cx={cx} cy={cy} r={6} fill="#1e1f22" stroke={color} stroke-width="2" stroke-dasharray="3 2" />
      ) : row.parents.length > 1 ? (
        <circle cx={cx} cy={cy} r={4} fill={color} stroke="#1e1f22" stroke-width="1.5" />
      ) : (
        <circle cx={cx} cy={cy} r={row.head ? 6 : 5.5} fill={row.head ? "#1e1f22" : color} stroke={color} stroke-width={row.head ? 3 : 0} />
      )}
    </svg>
  );
}

interface Badge {
  ref: RefLabel;
  remoteToo: boolean;
}

/** Merges `main` + `origin/main` on the same commit into one badge (cloud marker). */
function badges(refs: RefLabel[]): Badge[] {
  const locals = new Set(refs.filter((r) => r.kind === "branch").map((r) => r.name));
  const out: Badge[] = [];
  const merged = new Set<string>();
  for (const r of refs) {
    if (r.kind === "remote") {
      const short = r.name.split("/").slice(1).join("/");
      if (locals.has(short) && !merged.has(short)) {
        merged.add(short);
        continue;
      }
    }
  }
  const order = { head: 0, branch: 1, remote: 2, tag: 3 } as const;
  [...refs]
    .sort((a, b) => (b.current ? 1 : 0) - (a.current ? 1 : 0) || order[a.kind] - order[b.kind])
    .forEach((r) => {
      if (r.kind === "remote" && merged.has(r.name.split("/").slice(1).join("/"))) return;
      out.push({ ref: r, remoteToo: r.kind === "branch" && merged.has(r.name) });
    });
  return out;
}

export function RefBadge({ b, color, row }: { b: Badge; color: string; row?: GraphRow }) {
  const r = b.ref;
  const onMenu = (e: MouseEvent) => {
    if (r.kind === "branch") showMenu(e, branchMenu(r.name));
    else if (r.kind === "remote") showMenu(e, remoteMenu(r.name));
    else if (r.kind === "tag") showMenu(e, tagMenu(r.name, row?.sha ?? ""));
    else if (row) showMenu(e, commitMenu(row));
  };
  const onDbl = (e: MouseEvent) => {
    e.stopPropagation();
    if (r.kind === "branch" && !r.current) A.checkoutBranch(r.name);
    else if (r.kind === "remote") A.checkoutRemote(r.name);
  };
  const icon = r.kind === "tag" ? "tag" : r.kind === "remote" ? "cloud" : r.current ? "check" : "branch";
  return (
    <span
      class={`badge badge-${r.kind} ${r.current ? "current" : ""}`}
      style={r.kind === "tag" ? undefined : { "--c": color } as any}
      onContextMenu={onMenu}
      onDblClick={onDbl}
      title={r.kind === "branch" ? `Local branch ${r.name}${b.remoteToo ? " (in sync with remote)" : ""} — double-click to checkout` : r.name}
    >
      <Icon name={icon} size={11} />
      {r.name}
      {b.remoteToo && <Icon name="cloud" size={11} class="badge-cloud" />}
    </span>
  );
}

function WipText() {
  const s = useAppState();
  const n = s.status.length;
  const conflicts = s.status.filter((f) => f.conflicted).length;
  return (
    <span class="wip-text">
      <span class="wip-tag">// WIP</span>
      {s.info?.state !== "none" && <span class="state-tag">{s.info?.stateLabel}</span>}
      {n > 0 && (
        <span class="muted">
          {n} changed file{n === 1 ? "" : "s"}
        </span>
      )}
      {conflicts > 0 && <span class="conflict-tag">{conflicts} conflict{conflicts === 1 ? "" : "s"}</span>}
    </span>
  );
}

export function GraphView() {
  const s = useAppState();
  const rows = s.graph?.rows ?? [];
  const scrollRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(600);
  const [query, setQuery] = useState("");
  const [matchIdx, setMatchIdx] = useState(0);

  const graphW = Math.min(Math.max((s.graph?.maxLanes ?? 1) * LANE_W + PAD * 2, 60), 360);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setHeight(el.clientHeight));
    ro.observe(el);
    setHeight(el.clientHeight);
    return () => ro.disconnect();
  }, []);

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [] as number[];
    const out: number[] = [];
    rows.forEach((r, i) => {
      if (r.wip) return;
      if (
        r.subject.toLowerCase().includes(q) ||
        r.sha.startsWith(q) ||
        r.author.toLowerCase().includes(q) ||
        r.refs.some((x) => x.name.toLowerCase().includes(q))
      )
        out.push(i);
    });
    return out;
  }, [rows, query]);
  const matchSet = useMemo(() => new Set(matches), [matches]);

  const scrollToIndex = (i: number, center = true) => {
    const el = scrollRef.current;
    if (!el) return;
    const top = i * ROW_H;
    if (center) el.scrollTop = Math.max(0, top - el.clientHeight / 2 + ROW_H);
    else if (top < el.scrollTop) el.scrollTop = top;
    else if (top + ROW_H > el.scrollTop + el.clientHeight) el.scrollTop = top + ROW_H - el.clientHeight;
  };

  // External "scroll to commit" requests (sidebar, details panel).
  useEffect(() => {
    if (!s.scrollTo) return;
    const i = rows.findIndex((r) => r.sha === s.scrollTo!.sha);
    if (i >= 0) scrollToIndex(i);
  }, [s.scrollTo]);

  // Reset scroll when switching repository.
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [s.repo]);

  const jump = (dir: 1 | -1) => {
    if (!matches.length) return;
    const n = (matchIdx + dir + matches.length) % matches.length;
    setMatchIdx(n);
    setState({ selected: rows[matches[n]].sha });
    scrollToIndex(matches[n]);
  };

  const onKey = (e: KeyboardEvent) => {
    const i = rows.findIndex((r) => (r.wip ? "WIP" : r.sha) === getState().selected);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      const r = rows[n];
      if (r) {
        setState({ selected: r.wip ? "WIP" : r.sha });
        scrollToIndex(n, false);
      }
    }
  };

  const start = Math.max(0, Math.floor(scrollTop / ROW_H) - 8);
  const end = Math.min(rows.length, Math.ceil((scrollTop + height) / ROW_H) + 8);
  const visible = rows.slice(start, end);

  return (
    <div class="graph">
      <div class="graph-head" style={{ gridTemplateColumns: `${graphW}px 1fr 150px 130px 72px` }}>
        <span>Graph</span>
        <span class="graph-search">
          <Icon name="search" size={13} />
          <input
            type="text"
            placeholder="Search message, hash, author, ref…"
            value={query}
            spellcheck={false}
            onInput={(e) => {
              setQuery((e.target as HTMLInputElement).value);
              setMatchIdx(-1);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") jump(e.shiftKey ? -1 : 1);
              if (e.key === "Escape") setQuery("");
            }}
          />
          {query && (
            <span class="muted small">
              {matches.length ? `${Math.max(0, matchIdx) + 1}/${matches.length}` : "0"}
            </span>
          )}
        </span>
        <span>Author</span>
        <span>Date</span>
        <span>Hash</span>
      </div>
      <div class="graph-scroll" ref={scrollRef} tabIndex={0} onKeyDown={onKey} onScroll={(e) => setScrollTop((e.target as HTMLElement).scrollTop)}>
        <div style={{ height: px(rows.length * ROW_H), position: "relative" }}>
          {visible.map((r, k) => {
            const i = start + k;
            const id = r.wip ? "WIP" : r.sha;
            const sel = s.selected === id;
            const color = laneColor(r.color);
            return (
              <div
                key={id}
                class={`graph-row ${sel ? "selected" : ""} ${s.compare === id ? "selected2" : ""} ${r.head ? "head" : ""} ${matchSet.has(i) ? "match" : ""} ${r.wip ? "wip" : ""}`}
                style={{ top: px(i * ROW_H), gridTemplateColumns: `${graphW}px 1fr 150px 130px 72px` }}
                onMouseDown={(e) => {
                  if (e.button !== 0) return;
                  const st = getState();
                  // Ctrl+click marks a second commit to compare with the selected one.
                  if (e.ctrlKey && !r.wip && st.selected && st.selected !== "WIP" && st.selected !== id)
                    setState({ compare: st.compare === id ? null : id });
                  else setState({ selected: id, compare: null });
                }}
                onContextMenu={(e) => {
                  const st = getState();
                  if (!(st.compare && (st.selected === id || st.compare === id))) setState({ selected: id, compare: null });
                  if (!r.wip) showMenu(e, commitMenu(r));
                }}
                onDblClick={() => r.wip && setState({ focusCommitRequest: getState().focusCommitRequest + 1 })}
              >
                <GraphCell row={r} width={graphW} />
                <div class="graph-desc">
                  {r.wip ? (
                    <WipText />
                  ) : (
                    <>
                      {badges(r.refs).map((b) => (
                        <RefBadge key={b.ref.kind + b.ref.name} b={b} color={color} row={r} />
                      ))}
                      <span class="subject">{r.subject}</span>
                    </>
                  )}
                </div>
                <div class="graph-author">{!r.wip && <><Avatar name={r.author} size={18} /> <span>{r.author}</span></>}</div>
                <div class="graph-date">{!r.wip && formatDate(r.time)}</div>
                <div class="graph-sha mono">{!r.wip && r.sha.slice(0, 7)}</div>
              </div>
            );
          })}
        </div>
        {rows.length === 0 && s.graph && <div class="empty small">No commits yet. Make the first commit from the commit panel.</div>}
      </div>
    </div>
  );
}
