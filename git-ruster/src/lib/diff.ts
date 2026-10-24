// Line diff, intra-line (word) diff, 3-way merge regions and syntax highlighting helpers.
import { diffArrays, diffWordsWithSpace } from "diff";
import { diffIndices } from "node-diff3";
import hljs from "highlight.js/lib/common";

export interface Text {
  lines: string[];
  trailingNewline: boolean;
  eol: "\n" | "\r\n";
}

export function splitText(text: string): Text {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  let t = text.replace(/\r\n/g, "\n");
  const trailingNewline = t.endsWith("\n");
  if (trailingNewline) t = t.slice(0, -1);
  return { lines: t === "" && !trailingNewline ? [] : t.split("\n"), trailingNewline, eol };
}

export function joinText(t: { lines: string[]; trailingNewline: boolean; eol: string }): string {
  if (t.lines.length === 0) return "";
  return t.lines.join(t.eol) + (t.trailingNewline ? t.eol : "");
}

// ---------------------------------------------------------------------------------------------
// Two-way line diff
// ---------------------------------------------------------------------------------------------

export type BlockType = "eq" | "mod" | "add" | "del";

export interface Block {
  type: BlockType;
  aStart: number;
  aLen: number;
  bStart: number;
  bLen: number;
}

const normWs = (s: string) => s.replace(/\s+/g, "");

export function diffLinesToBlocks(a: string[], b: string[], ignoreWhitespace: boolean): Block[] {
  const eq = ignoreWhitespace ? (x: string, y: string) => normWs(x) === normWs(y) : (x: string, y: string) => x === y;
  // Common prefix / suffix keep the expensive part small.
  let pre = 0;
  while (pre < a.length && pre < b.length && eq(a[pre], b[pre])) pre++;
  let suf = 0;
  while (suf < a.length - pre && suf < b.length - pre && eq(a[a.length - 1 - suf], b[b.length - 1 - suf])) suf++;

  const blocks: Block[] = [];
  const push = (type: BlockType, aStart: number, aLen: number, bStart: number, bLen: number) => {
    if (aLen === 0 && bLen === 0) return;
    const last = blocks[blocks.length - 1];
    if (last && last.type === "eq" && type === "eq") {
      last.aLen += aLen;
      last.bLen += bLen;
      return;
    }
    blocks.push({ type, aStart, aLen, bStart, bLen });
  };
  push("eq", 0, pre, 0, pre);

  const am = a.slice(pre, a.length - suf);
  const bm = b.slice(pre, b.length - suf);
  let ai = pre;
  let bi = pre;
  if (am.length || bm.length) {
    const changes = diffArrays(am, bm, ignoreWhitespace ? { comparator: eq } : undefined);
    let delRun = 0;
    let addRun = 0;
    const flush = () => {
      if (delRun || addRun) {
        push(delRun && addRun ? "mod" : delRun ? "del" : "add", ai, delRun, bi, addRun);
        ai += delRun;
        bi += addRun;
        delRun = addRun = 0;
      }
    };
    for (const c of changes) {
      const n = c.count ?? c.value.length;
      if (c.removed) delRun += n;
      else if (c.added) addRun += n;
      else {
        flush();
        push("eq", ai, n, bi, n);
        ai += n;
        bi += n;
      }
    }
    flush();
  }
  push("eq", ai, suf, bi, suf);
  return blocks;
}

/** Character ranges per line: lineIndex -> [start, end) list. */
export type Ranges = Map<number, [number, number][]>;

/** Word-level differences inside a modified block. */
export function wordRanges(aLines: string[], bLines: string[], aStart: number, bStart: number, outA: Ranges, outB: Ranges) {
  const aText = aLines.join("\n");
  const bText = bLines.join("\n");
  if (aText.length + bText.length > 30000) return;
  const parts = diffWordsWithSpace(aText, bText);
  let ao = 0;
  let bo = 0;
  const mark = (out: Ranges, lines: string[], base: number, from: number, to: number) => {
    // Convert offsets in the joined block text to per-line ranges.
    let line = 0;
    let off = 0;
    while (line < lines.length && off + lines[line].length < from) {
      off += lines[line].length + 1;
      line++;
    }
    let pos = from;
    while (pos < to && line < lines.length) {
      const lineEnd = off + lines[line].length;
      const s = pos - off;
      const e = Math.min(to, lineEnd) - off;
      if (e > s) {
        const list = out.get(base + line) ?? [];
        list.push([s, e]);
        out.set(base + line, list);
      }
      pos = lineEnd + 1;
      off = lineEnd + 1;
      line++;
    }
  };
  for (const p of parts) {
    const len = p.value.length;
    if (p.removed) {
      mark(outA, aLines, aStart, ao, ao + len);
      ao += len;
    } else if (p.added) {
      mark(outB, bLines, bStart, bo, bo + len);
      bo += len;
    } else {
      ao += len;
      bo += len;
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Three-way merge regions
// ---------------------------------------------------------------------------------------------

export type ChunkType = "same" | "ours" | "theirs" | "both" | "conflict";

export interface Chunk {
  type: ChunkType;
  a: string[]; // ours
  o: string[]; // base
  b: string[]; // theirs
  aStart: number;
  bStart: number;
}

const sameLines = (x: string[], y: string[]) => x.length === y.length && x.every((v, i) => v === y[i]);

export function merge3(a: string[], o: string[], b: string[]): Chunk[] {
  interface Hunk {
    side: "a" | "b";
    oStart: number;
    oLen: number;
    sStart: number;
    sLen: number;
  }
  const hunks: Hunk[] = [
    ...diffIndices(o, a).map((h) => ({ side: "a" as const, oStart: h.buffer1[0], oLen: h.buffer1[1], sStart: h.buffer2[0], sLen: h.buffer2[1] })),
    ...diffIndices(o, b).map((h) => ({ side: "b" as const, oStart: h.buffer1[0], oLen: h.buffer1[1], sStart: h.buffer2[0], sLen: h.buffer2[1] })),
  ].sort((x, y) => x.oStart - y.oStart);

  const chunks: Chunk[] = [];
  let oPos = 0;
  let aPos = 0;
  let bPos = 0;
  const stable = (len: number) => {
    if (len <= 0) return;
    chunks.push({ type: "same", a: a.slice(aPos, aPos + len), o: o.slice(oPos, oPos + len), b: b.slice(bPos, bPos + len), aStart: aPos, bStart: bPos });
    oPos += len;
    aPos += len;
    bPos += len;
  };

  let i = 0;
  while (i < hunks.length) {
    const rStart = hunks[i].oStart;
    let rEnd = rStart + hunks[i].oLen;
    const group = [hunks[i++]];
    while (i < hunks.length && hunks[i].oStart <= rEnd) {
      rEnd = Math.max(rEnd, hunks[i].oStart + hunks[i].oLen);
      group.push(hunks[i++]);
    }
    stable(rStart - oPos);
    const range = (side: "a" | "b", pos: number): [number, number] => {
      const hs = group.filter((h) => h.side === side);
      if (!hs.length) return [pos, pos + (rEnd - rStart)];
      const minS = Math.min(...hs.map((h) => h.sStart));
      const maxE = Math.max(...hs.map((h) => h.sStart + h.sLen));
      const minO = Math.min(...hs.map((h) => h.oStart));
      const maxO = Math.max(...hs.map((h) => h.oStart + h.oLen));
      return [minS + (rStart - minO), maxE + (rEnd - maxO)];
    };
    const [as, ae] = range("a", aPos);
    const [bs, be] = range("b", bPos);
    const ca = a.slice(as, ae);
    const co = o.slice(rStart, rEnd);
    const cb = b.slice(bs, be);
    const type: ChunkType = sameLines(ca, co) ? "theirs" : sameLines(cb, co) ? "ours" : sameLines(ca, cb) ? "both" : "conflict";
    chunks.push({ type, a: ca, o: co, b: cb, aStart: as, bStart: bs });
    oPos = rEnd;
    aPos = ae;
    bPos = be;
  }
  stable(o.length - oPos);
  return chunks;
}

// ---------------------------------------------------------------------------------------------
// Syntax highlighting → per-line token lists
// ---------------------------------------------------------------------------------------------

export interface Token {
  text: string;
  cls: string;
}

const EXT_LANG: Record<string, string> = {
  rs: "rust", ts: "typescript", tsx: "typescript", mts: "typescript", cts: "typescript", js: "javascript", jsx: "javascript",
  mjs: "javascript", cjs: "javascript", py: "python", cs: "csharp", java: "java", kt: "kotlin", kts: "kotlin", go: "go",
  c: "c", h: "c", cpp: "cpp", hpp: "cpp", cc: "cpp", cxx: "cpp", json: "json", toml: "ini", ini: "ini", yml: "yaml",
  yaml: "yaml", md: "markdown", html: "xml", htm: "xml", xml: "xml", svg: "xml", vue: "xml", css: "css", scss: "scss",
  less: "less", sh: "bash", bash: "bash", ps1: "powershell", psm1: "powershell", sql: "sql", rb: "ruby", php: "php",
  swift: "swift", lua: "lua", pl: "perl", r: "r", diff: "diff", patch: "diff", gradle: "groovy", groovy: "groovy",
  dockerfile: "dockerfile", makefile: "makefile", cmake: "cmake", graphql: "graphql", proto: "protobuf", bat: "dos", cmd: "dos",
};

export function languageFor(path: string): string | null {
  const name = path.split("/").pop()!.toLowerCase();
  if (name === "dockerfile") return hljs.getLanguage("dockerfile") ? "dockerfile" : null;
  if (name === "makefile") return "makefile";
  const ext = name.includes(".") ? name.split(".").pop()! : "";
  const lang = EXT_LANG[ext] ?? ext;
  return lang && hljs.getLanguage(lang) ? lang : null;
}

const ENTITIES: Record<string, string> = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#x27;": "'", "&#39;": "'" };
const decode = (s: string) => s.replace(/&(amp|lt|gt|quot|#x27|#39);/g, (m) => ENTITIES[m] ?? m);

/** Highlights the whole text and splits the result into lines of tokens (null = no highlighting). */
export function highlightLines(lines: string[], lang: string | null): Token[][] | null {
  if (!lang || lines.length === 0) return null;
  const text = lines.join("\n");
  if (text.length > 500_000) return null;
  let html: string;
  try {
    html = hljs.highlight(text, { language: lang, ignoreIllegals: true }).value;
  } catch {
    return null;
  }
  const out: Token[][] = [[]];
  const stack: string[] = [];
  const re = /<span class="([^"]*)">|<\/span>|([^<]+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(html))) {
    if (m[1] !== undefined) stack.push(m[1]);
    else if (m[0] === "</span>") stack.pop();
    else {
      const cls = stack.join(" ");
      const parts = decode(m[2]).split("\n");
      parts.forEach((p, i) => {
        if (i > 0) out.push([]);
        if (p) out[out.length - 1].push({ text: p, cls });
      });
    }
  }
  return out.length === lines.length ? out : null;
}
