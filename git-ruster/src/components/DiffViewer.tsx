import { px } from "../lib/css";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { api, Blob } from "../api";
import { DiffFile, DiffRequest, getState, setState, Side } from "../state";
import { toastError } from "../ui";
import { Block, BlockType, diffLinesToBlocks, highlightLines, joinText, languageFor, Ranges, splitText, Text, Token, wordRanges } from "../lib/diff";
import { Icon } from "./Icon";
import { splitPath, statusInfo } from "./common";

const LINE_H = 20;
const CONTEXT = 4;

interface Loaded {
  left: Blob;
  right: Blob;
  a: Text;
  b: Text;
}

interface Row {
  kind: "line" | "fold";
  a: number | null;
  b: number | null;
  type: BlockType;
  block: number;
  first?: boolean;
  fold?: number; // hidden line count
}

/** The same file with left and right exchanged. */
function swapFile(f: DiffFile): DiffFile {
  const status = f.status === "A" ? "D" : f.status === "D" ? "A" : f.status;
  return {
    ...f,
    status,
    path: f.oldPath ?? f.path,
    oldPath: f.oldPath ? f.path : f.oldPath,
    left: f.right,
    right: f.left,
    leftLabel: f.rightLabel,
    rightLabel: f.leftLabel,
  };
}

async function readSide(repo: string, side: Side): Promise<Blob> {
  switch (side.kind) {
    case "empty":
      return { exists: false, binary: false, tooLarge: false, text: "" };
    case "rev":
      return api.readBlob(repo, `${side.rev}:${side.path}`);
    case "index":
      return api.readBlob(repo, `:${side.path}`);
    case "work":
      return api.readWorkFile(repo, side.path);
  }
}

/** Display width of a line in character cells (tabs expand to the next multiple of 4). */
function visualCols(line: string) {
  if (!line.includes("\t")) return line.length;
  let c = 0;
  for (const ch of line) c = ch === "\t" ? c + 4 - (c % 4) : c + 1;
  return c;
}

function pref<T>(key: string, def: T): T {
  try {
    const v = localStorage.getItem("git-ruster:" + key);
    return v === null ? def : (JSON.parse(v) as T);
  } catch {
    return def;
  }
}
function setPref(key: string, v: unknown) {
  try {
    localStorage.setItem("git-ruster:" + key, JSON.stringify(v));
  } catch {
    /* ignore */
  }
}

/** Renders a line from syntax tokens, wrapping changed character ranges. */
function LineContent({ text, tokens, ranges }: { text: string; tokens?: Token[]; ranges?: [number, number][] }) {
  const segs = tokens && tokens.length ? tokens : [{ text, cls: "" }];
  if (!ranges || !ranges.length) return <>{segs.map((t, i) => (t.cls ? <span key={i} class={t.cls}>{t.text}</span> : t.text))}</>;
  const out: any[] = [];
  let pos = 0;
  let k = 0;
  for (const t of segs) {
    let s = 0;
    while (s < t.text.length) {
      const abs = pos + s;
      const inR = ranges.find(([a, b]) => abs >= a && abs < b);
      let end: number;
      if (inR) end = Math.min(t.text.length, inR[1] - pos);
      else {
        const next = ranges.filter(([a]) => a > abs).reduce((m, [a]) => Math.min(m, a), Infinity);
        end = Math.min(t.text.length, next - pos);
      }
      const piece = t.text.slice(s, end);
      const node = t.cls ? <span class={t.cls}>{piece}</span> : piece;
      out.push(inR ? <span key={k++} class="w-hl">{node}</span> : <span key={k++}>{node}</span>);
      s = end;
    }
    pos += t.text.length;
  }
  return <>{out}</>;
}

export function DiffViewer({ req }: { req: DiffRequest }) {
  const repo = getState().repo!;
  const [index, setIndex] = useState(req.index);
  const [swapped, setSwapped] = useState(false);
  const file = swapped ? swapFile(req.files[index]) : req.files[index];
  const [data, setData] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<"split" | "unified">(() => pref("diffMode", "split"));
  const [ignoreWs, setIgnoreWs] = useState<boolean>(() => pref("diffIgnoreWs", false));
  const [collapse, setCollapse] = useState<boolean>(() => pref("diffCollapse", true));
  const [wrap, setWrap] = useState<boolean>(() => pref("diffWrap", false));
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [current, setCurrent] = useState(-1);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewH, setViewH] = useState(600);
  const [viewW, setViewW] = useState(1200);
  const [hx, setHx] = useState(0);
  const [charW, setCharW] = useState(7.8);
  const [reload, setReload] = useState(0);
  const bodyRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLSpanElement>(null);
  const hbarRef = useRef<HTMLDivElement>(null);

  const close = () => setState({ diff: null });

  useEffect(() => {
    let alive = true;
    setError(null);
    if (reload === 0) setData(null);
    Promise.all([readSide(repo, file.left), readSide(repo, file.right)])
      .then(([left, right]) => {
        if (!alive) return;
        setData({ left, right, a: splitText(left.text), b: splitText(right.text) });
      })
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [index, reload, swapped]);

  useEffect(() => {
    setExpanded(new Set());
    setCurrent(-1);
    setHx(0);
    if (bodyRef.current) bodyRef.current.scrollTop = 0;
  }, [index]);

  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    const measure = () => (setViewH(el.clientHeight), setViewW(el.clientWidth));
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    if (measureRef.current) setCharW(measureRef.current.getBoundingClientRect().width / 100 || 7.8);
    return () => ro.disconnect();
  }, [data !== null]);

  const lang = useMemo(() => languageFor(file.path), [file.path]);
  const model = useMemo(() => {
    if (!data || data.left.binary || data.right.binary || data.left.tooLarge || data.right.tooLarge) return null;
    const A = data.a.lines;
    const B = data.b.lines;
    const blocks = diffLinesToBlocks(A, B, ignoreWs);
    const ra: Ranges = new Map();
    const rb: Ranges = new Map();
    for (const bl of blocks)
      if (bl.type === "mod" && bl.aLen + bl.bLen <= 800)
        wordRanges(A.slice(bl.aStart, bl.aStart + bl.aLen), B.slice(bl.bStart, bl.bStart + bl.bLen), bl.aStart, bl.bStart, ra, rb);
    return { blocks, ra, rb, ta: highlightLines(A, lang), tb: highlightLines(B, lang) };
  }, [data, ignoreWs, lang]);

  const rows = useMemo(() => {
    if (!model) return [] as Row[];
    const out: Row[] = [];
    model.blocks.forEach((bl: Block, bi) => {
      if (bl.type === "eq") {
        const n = bl.aLen;
        const head = bi === 0 ? 0 : CONTEXT;
        const tail = bi === model.blocks.length - 1 ? 0 : CONTEXT;
        const foldable = collapse && !expanded.has(bi) && n > head + tail + 2;
        for (let k = 0; k < n; k++) {
          if (foldable && k === head) {
            out.push({ kind: "fold", a: null, b: null, type: "eq", block: bi, fold: n - head - tail });
            k = n - tail - 1;
            continue;
          }
          out.push({ kind: "line", a: bl.aStart + k, b: bl.bStart + k, type: "eq", block: bi });
        }
      } else if (mode === "split") {
        const n = Math.max(bl.aLen, bl.bLen);
        for (let k = 0; k < n; k++)
          out.push({ kind: "line", a: k < bl.aLen ? bl.aStart + k : null, b: k < bl.bLen ? bl.bStart + k : null, type: bl.type, block: bi, first: k === 0 });
      } else {
        for (let k = 0; k < bl.aLen; k++) out.push({ kind: "line", a: bl.aStart + k, b: null, type: bl.type, block: bi, first: k === 0 });
        for (let k = 0; k < bl.bLen; k++) out.push({ kind: "line", a: null, b: bl.bStart + k, type: bl.type, block: bi, first: k === 0 && bl.aLen === 0 });
      }
    });
    return out;
  }, [model, mode, collapse, expanded]);

  // Width of one text column: the grid's fixed columns (see .d-row in styles.css) and the text padding.
  const textW = Math.max(50, (mode === "split" ? (viewW - 122) / 2 : viewW - 116) - 6);
  const wrapCols = Math.max(10, Math.floor((textW - 2) / charW));

  /** Row tops in px (length rows.length + 1); rows grow when wrapped. */
  const tops = useMemo(() => {
    const t = new Array<number>(rows.length + 1);
    t[0] = 0;
    rows.forEach((r, i) => {
      let lines = 1;
      if (wrap && data && r.kind === "line") {
        const a = r.a !== null ? visualCols(data.a.lines[r.a]) : 0;
        const b = r.b !== null ? visualCols(data.b.lines[r.b]) : 0;
        lines = Math.max(1, Math.ceil((mode === "split" ? Math.max(a, b) : r.b === null ? a : b) / wrapCols));
      }
      t[i + 1] = t[i] + lines * LINE_H;
    });
    return t;
  }, [rows, wrap, wrapCols, data, mode]);
  const totalH = tops[rows.length];
  /** Index of the row at `y` px. */
  const rowAt = (y: number) => {
    let lo = 0;
    let hi = rows.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (tops[mid + 1] <= y) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  };

  const changeRows = useMemo(() => rows.map((r, i) => (r.first ? i : -1)).filter((i) => i >= 0), [rows]);

  const goChange = (dir: 1 | -1) => {
    if (!changeRows.length) return;
    let n = current + dir;
    if (n < 0 || n >= changeRows.length) {
      // At the end of the file: continue with the next/previous file, like JetBrains.
      if (dir === 1 && index < req.files.length - 1) return setIndex(index + 1);
      if (dir === -1 && index > 0) return setIndex(index - 1);
      n = Math.max(0, Math.min(changeRows.length - 1, n));
    }
    setCurrent(n);
    const el = bodyRef.current;
    if (el) el.scrollTop = Math.max(0, tops[changeRows[n]] - el.clientHeight / 3);
  };

  // Jump to the first change when a file is opened.
  useEffect(() => {
    if (current === -1 && changeRows.length && bodyRef.current) {
      setCurrent(0);
      bodyRef.current.scrollTop = Math.max(0, tops[changeRows[0]] - bodyRef.current.clientHeight / 3);
    }
  }, [changeRows]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if (e.key === "Escape") close();
      else if (e.key === "F7" || (e.altKey && e.key === "ArrowDown")) {
        e.preventDefault();
        goChange(e.shiftKey ? -1 : 1);
      } else if (e.altKey && e.key === "ArrowUp") {
        e.preventDefault();
        goChange(-1);
      } else if (e.altKey && e.key === "ArrowRight" && index < req.files.length - 1) setIndex(index + 1);
      else if (e.altKey && e.key === "ArrowLeft" && index > 0) setIndex(index - 1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const canRevert = file.right.kind === "work" && !!data?.right.exists;
  const revertBlock = async (bl: Block) => {
    if (!data) return;
    const lines = [...data.b.lines.slice(0, bl.bStart), ...data.a.lines.slice(bl.aStart, bl.aStart + bl.aLen), ...data.b.lines.slice(bl.bStart + bl.bLen)];
    try {
      await api.writeWorkFile(repo, file.path, joinText({ ...data.b, lines }));
      setReload((n) => n + 1);
    } catch (e) {
      toastError(e);
    }
  };

  const maxLen = useMemo(() => {
    if (!data) return 0;
    let m = 0;
    for (const l of data.a.lines) m = Math.max(m, visualCols(l));
    for (const l of data.b.lines) m = Math.max(m, visualCols(l));
    return m;
  }, [data]);
  const maxHx = wrap ? 0 : Math.max(0, Math.ceil(maxLen * charW - textW + 20));
  const onWheel = (e: WheelEvent) => {
    const dx = e.shiftKey ? e.deltaY : e.deltaX;
    if (dx && (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY))) {
      e.preventDefault();
      setHx((x) => Math.max(0, Math.min(maxHx, x + dx)));
    }
  };
  // Keep the horizontal scrollbar in sync with wheel panning; clamp after resizes.
  useEffect(() => {
    if (hx > maxHx) setHx(maxHx);
    if (hbarRef.current && Math.abs(hbarRef.current.scrollLeft - hx) > 1) hbarRef.current.scrollLeft = hx;
  }, [hx, maxHx]);
  const shift = wrap ? 0 : hx;

  const start = Math.max(0, rowAt(scrollTop) - 10);
  const end = Math.min(rows.length, rowAt(scrollTop + viewH) + 11);

  const textCell = (side: "a" | "b", r: Row) => {
    const idx = side === "a" ? r.a : r.b;
    if (idx === null || !data || !model) return <div class={`d-text filler`} />;
    const line = (side === "a" ? data.a.lines : data.b.lines)[idx];
    const tokens = (side === "a" ? model.ta : model.tb)?.[idx];
    const ranges = (side === "a" ? model.ra : model.rb).get(idx);
    return (
      <div class="d-text">
        <div class={`d-text-inner ${wrap ? "wrap" : ""}`} style={shift ? { transform: `translateX(${-shift}px)` } : undefined}>
          <LineContent text={line} tokens={tokens} ranges={ranges} />
        </div>
      </div>
    );
  };

  const typeCls = (r: Row, side: "a" | "b") => {
    if (r.type === "eq") return "";
    if (mode === "unified") return side === "a" ? "t-del" : "t-add";
    return r.type === "mod" ? "t-mod" : r.type === "add" ? "t-add" : "t-del";
  };

  const st = statusInfo(file.status);
  const blocks = model?.blocks ?? [];
  const nChanges = blocks.filter((b) => b.type !== "eq").length;
  const noContent = data && !model;

  return (
    <div class="overlay diff-viewer">
      <div class="ov-head">
        <div class="ov-title">
          <span class={`st-letter ${st.cls}`}>{file.status}</span>
          <b>{file.path}</b>
          {file.oldPath && <span class="muted"> (renamed from {file.oldPath})</span>}
          <span class="muted"> — {req.title}</span>
        </div>
        <div class="ov-tools">
          <button class="icon-btn" title="Previous change (Shift+F7)" onClick={() => goChange(-1)}>
            <Icon name="up" size={16} />
          </button>
          <button class="icon-btn" title="Next change (F7)" onClick={() => goChange(1)}>
            <Icon name="down" size={16} />
          </button>
          <span class="muted small counter">{nChanges ? `${Math.max(current, 0) + 1} / ${nChanges} changes` : "no changes"}</span>
          <span class="tb-sep" />
          <button class="icon-btn" title="Previous file (Alt+Left)" disabled={index === 0} onClick={() => setIndex(index - 1)}>
            <Icon name="left" size={16} />
          </button>
          <span class="muted small counter">
            {index + 1} / {req.files.length}
          </span>
          <button class="icon-btn" title="Next file (Alt+Right)" disabled={index >= req.files.length - 1} onClick={() => setIndex(index + 1)}>
            <Icon name="right" size={16} />
          </button>
          <span class="tb-sep" />
          <button class="icon-btn" title="Swap sides" onClick={() => (setSwapped(!swapped), setCurrent(-1))}>
            <Icon name="swap" size={16} />
          </button>
          <div class="seg">
            <button class={mode === "split" ? "on" : ""} onClick={() => (setMode("split"), setPref("diffMode", "split"))}>
              Side-by-side
            </button>
            <button class={mode === "unified" ? "on" : ""} onClick={() => (setMode("unified"), setPref("diffMode", "unified"))}>
              Unified
            </button>
          </div>
          <label class="check">
            <input type="checkbox" checked={ignoreWs} onChange={(e) => (setIgnoreWs((e.target as HTMLInputElement).checked), setPref("diffIgnoreWs", (e.target as HTMLInputElement).checked))} />
            Ignore whitespace
          </label>
          <label class="check">
            <input type="checkbox" checked={collapse} onChange={(e) => (setCollapse((e.target as HTMLInputElement).checked), setExpanded(new Set()), setPref("diffCollapse", (e.target as HTMLInputElement).checked))} />
            Collapse unchanged
          </label>
          <label class="check">
            <input type="checkbox" checked={wrap} onChange={(e) => (setWrap((e.target as HTMLInputElement).checked), setPref("diffWrap", (e.target as HTMLInputElement).checked))} />
            Wrap lines
          </label>
          <button class="icon-btn" title="Close (Esc)" onClick={close}>
            <Icon name="x" size={18} />
          </button>
        </div>
      </div>
      <div class="ov-body">
        {req.files.length > 1 && (
          <div class="ov-files">
            {req.files.map((f, i) => {
              const { name, dir } = splitPath(f.path);
              const s = statusInfo(f.status);
              return (
                <div key={f.path} class={`file-row ${i === index ? "selected" : ""}`} onClick={() => setIndex(i)} title={f.path}>
                  <span class={`file-name ${s.cls}`}>{name}</span>
                  <span class="file-dir">{dir}</span>
                </div>
              );
            })}
          </div>
        )}
        <div class="diff-main">
          {mode === "split" ? (
            <div class="diff-cols-head">
              <div>
                <b>{file.leftLabel}</b> <span class="muted">{file.oldPath ?? file.path}</span>
                {data && !data.left.exists && <span class="muted"> (does not exist)</span>}
              </div>
              <div>
                <b>{file.rightLabel}</b> <span class="muted">{file.path}</span>
                {data && !data.right.exists && <span class="muted"> (does not exist)</span>}
              </div>
            </div>
          ) : (
            <div class="diff-cols-head single">
              <div>
                <b>{file.leftLabel}</b> → <b>{file.rightLabel}</b>
              </div>
            </div>
          )}
          <span ref={measureRef} class="d-measure">
            {"0".repeat(100)}
          </span>
          {error && <div class="empty">{error}</div>}
          {!data && !error && <div class="empty muted">Loading…</div>}
          {noContent && (
            <div class="empty">
              {data!.left.binary || data!.right.binary ? "Binary files differ" : "File is too large to display"}
            </div>
          )}
          {model && (
            <div class="diff-body-wrap">
              <div class="diff-body-col">
                <div class={`diff-body ${mode}`} ref={bodyRef} onScroll={(e) => setScrollTop((e.target as HTMLElement).scrollTop)} onWheel={onWheel}>
                  <div style={{ height: px(totalH), position: "relative" }}>
                    {rows.slice(start, end).map((r, k) => {
                      const i = start + k;
                      const h = tops[i + 1] - tops[i];
                      const top = px(tops[i]);
                      const isCur = current >= 0 && blocks[r.block] && changeRows[current] !== undefined && rows[changeRows[current]]?.block === r.block;
                      if (r.kind === "fold")
                        return (
                          <div key={i} class="d-row d-fold" style={{ top }} onClick={() => setExpanded(new Set([...expanded, r.block]))}>
                            <Icon name="diff" size={12} /> {r.fold} unchanged lines — click to expand
                          </div>
                        );
                      if (mode === "split")
                        return (
                          <div key={i} class={`d-row ${isCur ? "cur" : ""}`} style={{ top, height: px(h) }}>
                            <div class={`d-ln ${typeCls(r, "a")}`}>{r.a !== null ? r.a + 1 : ""}</div>
                            <div class={`d-side ${typeCls(r, "a")} ${r.a === null && r.type !== "eq" ? "pad" : ""}`}>{textCell("a", r)}</div>
                            <div class="d-gutter">
                              {r.first && canRevert && (
                                <button class="revert-btn" title="Revert this change" onClick={() => revertBlock(blocks[r.block])}>
                                  »
                                </button>
                              )}
                            </div>
                            <div class={`d-ln ${typeCls(r, "b")}`}>{r.b !== null ? r.b + 1 : ""}</div>
                            <div class={`d-side ${typeCls(r, "b")} ${r.b === null && r.type !== "eq" ? "pad" : ""}`}>{textCell("b", r)}</div>
                          </div>
                        );
                      const side = r.a !== null && r.b === null ? "a" : "b";
                      return (
                        <div key={i} class={`d-row unified ${isCur ? "cur" : ""}`} style={{ top, height: px(h) }}>
                          <div class={`d-ln ${typeCls(r, side)}`}>{r.a !== null ? r.a + 1 : ""}</div>
                          <div class={`d-ln ${typeCls(r, side)}`}>{r.b !== null ? r.b + 1 : ""}</div>
                          <div class={`d-mark ${typeCls(r, side)}`}>
                            {r.first && canRevert ? (
                              <button class="revert-btn" title="Revert this change" onClick={() => revertBlock(blocks[r.block])}>
                                »
                              </button>
                            ) : r.type === "eq" ? "" : side === "a" ? "−" : "+"}
                          </div>
                          <div class={`d-side ${typeCls(r, side)}`}>{textCell(side, r)}</div>
                        </div>
                      );
                    })}
                  </div>
                </div>
                {maxHx > 0 && (
                  <div class="d-hscroll" ref={hbarRef} onScroll={(e) => setHx(Math.min(maxHx, (e.target as HTMLElement).scrollLeft))}>
                    <div style={{ width: `calc(100% + ${px(maxHx)})`, height: "1px" }} />
                  </div>
                )}
              </div>
              <div class="d-ruler">
                {rows.length > 0 &&
                  changeRows.map((ri) => {
                    const bl = blocks[rows[ri].block];
                    const n = mode === "split" ? Math.max(bl.aLen, bl.bLen) : bl.aLen + bl.bLen;
                    const h = tops[Math.min(rows.length, ri + n)] - tops[ri];
                    return (
                      <div
                        key={ri}
                        class={`d-ruler-mark t-${bl.type}`}
                        style={{ top: `${(tops[ri] / totalH) * 100}%`, height: `max(3px, ${(h / totalH) * 100}%)` }}
                        onClick={() => bodyRef.current && (bodyRef.current.scrollTop = tops[ri] - viewH / 3)}
                      />
                    );
                  })}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
