import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { api, ConflictVersions } from "../api";
import * as A from "../actions";
import { conflictLabels, getState, MergeRequest, setState, useAppState } from "../state";
import { confirm, toastError } from "../ui";
import { Chunk, joinText, merge3, splitText } from "../lib/diff";
import { Icon } from "./Icon";

type SideState = "pending" | "applied" | "ignored";

interface Res {
  left: SideState;
  right: SideState;
  /** Manually edited result lines (overrides left/right composition). */
  edited: string[] | null;
}

const FOLD_CONTEXT = 3;

function initialRes(c: Chunk, autoApply: boolean): Res {
  switch (c.type) {
    case "ours":
      return { left: autoApply ? "applied" : "pending", right: "ignored", edited: null };
    case "theirs":
      return { left: "ignored", right: autoApply ? "applied" : "pending", edited: null };
    case "both":
      return { left: autoApply ? "applied" : "pending", right: autoApply ? "ignored" : "pending", edited: null };
    default:
      return { left: "pending", right: "pending", edited: null };
  }
}

function resultLines(c: Chunk, r: Res): string[] {
  if (c.type === "same") return c.o;
  if (r.edited) return r.edited;
  const la = r.left === "applied";
  const lb = r.right === "applied";
  if (c.type === "both") return la || lb ? c.a : c.o;
  if (la && lb) return [...c.a, ...c.b];
  if (la) return c.a;
  if (lb) return c.b;
  return c.o;
}

function isResolved(c: Chunk, r: Res) {
  if (c.type === "same") return true;
  if (r.edited) return true;
  return r.left !== "pending" && r.right !== "pending";
}

function AutoTextarea({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const rows = Math.max(1, value.split("\n").length);
  useEffect(() => {
    if (ref.current) ref.current.style.height = `${rows * 19 + 2}px`;
  }, [rows]);
  return <textarea ref={ref} class="m-edit" spellcheck={false} wrap="off" value={value} onInput={(e) => onChange((e.target as HTMLTextAreaElement).value)} />;
}

function Lines({ lines, start, cls }: { lines: string[]; start?: number; cls?: string }) {
  return (
    <div class={`m-lines ${cls ?? ""}`}>
      {lines.map((l, i) => (
        <div class="m-line" key={i}>
          {start !== undefined && <span class="m-ln">{start + i + 1}</span>}
          <span class="m-txt">{l || " "}</span>
        </div>
      ))}
      {lines.length === 0 && <div class="m-line empty-line" />}
    </div>
  );
}

export function MergeTool({ req }: { req: MergeRequest }) {
  const s = useAppState();
  const repo = getState().repo!;
  const info = s.info;
  const labels = conflictLabels();
  const [v, setV] = useState<ConflictVersions | null>(null);
  const [res, setRes] = useState<Res[]>([]);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [cur, setCur] = useState(0);
  const bodyRef = useRef<HTMLDivElement>(null);

  const close = () => setState({ merge: null });

  useEffect(() => {
    api.conflictVersions(repo, req.path).then(setV).catch(toastError);
  }, [req.path]);

  const texts = useMemo(() => (v ? { a: splitText(v.ours.text), o: splitText(v.base.text), b: splitText(v.theirs.text), w: splitText(v.work.text) } : null), [v]);
  const chunks = useMemo(() => (texts ? merge3(texts.a.lines, texts.o.lines, texts.b.lines) : []), [texts]);

  useEffect(() => {
    setRes(chunks.map((c) => initialRes(c, true)));
    setExpanded(new Set());
  }, [chunks]);

  const changeIdx = chunks.map((c, i) => (c.type !== "same" ? i : -1)).filter((i) => i >= 0);
  const conflictIdx = chunks.map((c, i) => (c.type === "conflict" ? i : -1)).filter((i) => i >= 0);
  const unresolved = res.length ? changeIdx.filter((i) => !isResolved(chunks[i], res[i])).length : 0;
  const unresolvedConflicts = res.length ? conflictIdx.filter((i) => !isResolved(chunks[i], res[i])).length : 0;

  const upd = (i: number, patch: Partial<Res>) => setRes((rs) => rs.map((r, k) => (k === i ? { ...r, ...patch } : r)));

  const applyAll = (side: "left" | "right") =>
    setRes((rs) =>
      rs.map((r, i) => {
        const c = chunks[i];
        if (c.type === "same" || isResolved(c, r)) return r;
        return side === "left" ? { ...r, left: "applied", right: "ignored" } : { ...r, left: "ignored", right: "applied" };
      }),
    );
  const applyNonConflicting = () =>
    setRes((rs) => rs.map((r, i) => (chunks[i].type !== "conflict" && chunks[i].type !== "same" && !isResolved(chunks[i], r) ? initialRes(chunks[i], true) : r)));

  const scrollToChunk = (i: number) => {
    const el = bodyRef.current?.querySelector(`[data-chunk="${i}"]`) as HTMLElement | null;
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  };
  const nav = (dir: 1 | -1) => {
    const list = changeIdx.filter((i) => !isResolved(chunks[i], res[i]));
    const pool = list.length ? list : changeIdx;
    if (!pool.length) return;
    const next = dir === 1 ? pool.find((i) => i > cur) ?? pool[0] : [...pool].reverse().find((i) => i < cur) ?? pool[pool.length - 1];
    setCur(next);
    scrollToChunk(next);
  };

  useEffect(() => {
    if (changeIdx.length && res.length) {
      const first = conflictIdx[0] ?? changeIdx[0];
      setCur(first);
      setTimeout(() => scrollToChunk(first), 50);
    }
  }, [res.length]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).tagName === "TEXTAREA") return;
      if (e.key === "Escape") close();
      if (e.key === "F7") {
        e.preventDefault();
        nav(e.shiftKey ? -1 : 1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const save = async () => {
    if (!texts) return;
    if (unresolved > 0 && !(await confirm("Unresolved changes", `${unresolved} change(s) are not resolved yet. Save the result anyway?`, "Save", true))) return;
    const lines = chunks.flatMap((c, i) => resultLines(c, res[i]));
    const content = joinText({ lines, trailingNewline: texts.a.trailingNewline || texts.b.trailingNewline, eol: texts.w.eol });
    try {
      await api.writeWorkFile(repo, req.path, content);
      await A.markResolved(req.path);
      close();
    } catch (e) {
      toastError(e);
    }
  };

  const fileChange = s.status.find((f) => f.path === req.path);
  const header = (
    <div class="ov-head">
      <div class="ov-title">
        <Icon name="merge" size={16} />
        <b>Merge Revisions for {req.path}</b>
        {info && <span class="muted"> — {info.stateLabel}</span>}
      </div>
      <div class="ov-tools">
        <span class={`counter ${unresolvedConflicts ? "warn" : "ok"}`}>
          {unresolvedConflicts ? `${unresolvedConflicts} conflict${unresolvedConflicts === 1 ? "" : "s"}` : "No conflicts"}
          {unresolved - unresolvedConflicts > 0 ? `, ${unresolved - unresolvedConflicts} change(s) left` : ""}
        </span>
        <button class="icon-btn" title="Previous change (Shift+F7)" onClick={() => nav(-1)}>
          <Icon name="up" size={16} />
        </button>
        <button class="icon-btn" title="Next change (F7)" onClick={() => nav(1)}>
          <Icon name="down" size={16} />
        </button>
        <span class="tb-sep" />
        <button onClick={applyNonConflicting} title="Apply all non-conflicting changes">
          Apply non-conflicting
        </button>
        <button onClick={() => applyAll("left")}>Accept Left</button>
        <button onClick={() => applyAll("right")}>Accept Right</button>
        <span class="tb-sep" />
        <button onClick={close}>Cancel</button>
        <button class="primary" onClick={save}>
          Apply
        </button>
      </div>
    </div>
  );

  if (v && texts && (!v.ours.exists || !v.theirs.exists || v.ours.binary || v.theirs.binary)) {
    const deletedOurs = !v.ours.exists;
    return (
      <div class="overlay merge-tool">
        {header}
        <div class="empty">
          <div class="empty-title">
            {v.ours.binary || v.theirs.binary
              ? "Binary file conflict"
              : deletedOurs
                ? `Deleted in ${labels.ours}, modified in ${labels.theirs}`
                : `Modified in ${labels.ours}, deleted in ${labels.theirs}`}
          </div>
          <div class="row-gap">
            <button onClick={() => fileChange && (A.acceptSide(fileChange, "ours"), close())}>Use {labels.ours}</button>
            <button onClick={() => fileChange && (A.acceptSide(fileChange, "theirs"), close())}>Use {labels.theirs}</button>
          </div>
        </div>
      </div>
    );
  }

  let resultLine = 0;
  return (
    <div class="overlay merge-tool">
      {header}
      <div class="m-cols-head">
        <div>
          <b>{labels.ours}</b> <span class="muted">(left)</span>
        </div>
        <div />
        <div>
          <b>Result</b> <span class="muted">(editable)</span>
        </div>
        <div />
        <div>
          <b>{labels.theirs}</b> <span class="muted">(right)</span>
        </div>
      </div>
      {!v && <div class="empty muted">Loading…</div>}
      {v && res.length === chunks.length && (
        <div class="m-body" ref={bodyRef}>
          {chunks.map((c, i) => {
            const r = res[i];
            const out = resultLines(c, r);
            const rStart = resultLine;
            resultLine += out.length;
            if (c.type === "same") {
              const n = c.o.length;
              const head = i === 0 ? 0 : FOLD_CONTEXT;
              const tail = i === chunks.length - 1 ? 0 : FOLD_CONTEXT;
              if (!expanded.has(i) && n > head + tail + 2) {
                const cut = (l: string[]) => [l.slice(0, head), l.slice(n - tail)];
                const [ah, at] = cut(c.a);
                const [rh, rt] = cut(c.o);
                const [bh, bt] = cut(c.b);
                return (
                  <div class="m-chunk same" key={i} data-chunk={i}>
                    <Lines lines={ah} start={c.aStart} />
                    <div />
                    <Lines lines={rh} start={rStart} />
                    <div />
                    <Lines lines={bh} start={c.bStart} />
                    <div class="m-fold" onClick={() => setExpanded(new Set([...expanded, i]))}>
                      {n - head - tail} unchanged lines — click to expand
                    </div>
                    <Lines lines={at} start={c.aStart + n - tail} />
                    <div />
                    <Lines lines={rt} start={rStart + n - tail} />
                    <div />
                    <Lines lines={bt} start={c.bStart + n - tail} />
                  </div>
                );
              }
              return (
                <div class="m-chunk same" key={i} data-chunk={i}>
                  <Lines lines={c.a} start={c.aStart} />
                  <div />
                  <Lines lines={c.o} start={rStart} />
                  <div />
                  <Lines lines={c.b} start={c.bStart} />
                </div>
              );
            }
            const resolved = isResolved(c, r);
            const cls = `m-chunk change t-${c.type} ${resolved ? "resolved" : "unresolved"} ${cur === i ? "cur" : ""}`;
            const leftRelevant = c.type !== "theirs";
            const rightRelevant = c.type !== "ours" && c.type !== "both";
            const sideBtns = (side: "left" | "right") => {
              const st = r[side];
              const other = side === "left" ? r.right : r.left;
              const relevant = side === "left" ? leftRelevant : rightRelevant;
              if (!relevant || r.edited) return null;
              const arrow = side === "left" ? "»" : "«";
              if (st === "pending")
                return (
                  <>
                    <button class="m-btn apply" title={side === "left" ? "Accept left" : "Accept right"} onClick={() => upd(i, side === "left" ? { left: "applied", right: c.type === "conflict" ? (other === "applied" ? "applied" : "ignored") : r.right } : { right: "applied", left: c.type === "conflict" ? (other === "applied" ? "applied" : "ignored") : r.left })}>
                      {arrow}
                    </button>
                    <button class="m-btn ignore" title="Ignore" onClick={() => upd(i, { [side]: "ignored" } as Partial<Res>)}>
                      ×
                    </button>
                  </>
                );
              if (st === "ignored" && c.type === "conflict" && other === "applied")
                return (
                  <button class="m-btn append" title={side === "left" ? "Append left as well" : "Append right as well"} onClick={() => upd(i, { [side]: "applied" } as Partial<Res>)}>
                    +
                  </button>
                );
              return (
                <button class="m-btn undo" title="Undo" onClick={() => upd(i, initialRes(c, false))}>
                  ↺
                </button>
              );
            };
            return (
              <div class={cls} key={i} data-chunk={i} onClick={() => setCur(i)}>
                <Lines lines={c.a} start={c.aStart} cls={`side-a ${r.left}`} />
                <div class="m-gutter">{sideBtns("left")}</div>
                <div class="m-result">
                  <AutoTextarea value={out.join("\n")} onChange={(t) => upd(i, { edited: t === "" ? [] : t.split("\n") })} />
                  {r.edited && (
                    <button class="m-btn undo small-abs" title="Discard manual edit" onClick={() => upd(i, { edited: null })}>
                      ↺
                    </button>
                  )}
                </div>
                <div class="m-gutter">{sideBtns("right")}</div>
                <Lines lines={c.b} start={c.bStart} cls={`side-b ${r.right}`} />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
