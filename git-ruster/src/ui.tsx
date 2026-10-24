// Global UI primitives with an imperative API: context menus, form dialogs, toasts.
import { px } from "./lib/css";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";

// ---------------------------------------------------------------------------------------------
// Tiny observable
// ---------------------------------------------------------------------------------------------

type Listener = () => void;
function observable<T>(initial: T) {
  let value = initial;
  const listeners = new Set<Listener>();
  return {
    get: () => value,
    set(v: T) {
      value = v;
      listeners.forEach((l) => l());
    },
    use(): T {
      const [, force] = useState(0);
      useEffect(() => {
        const l = () => force((n) => n + 1);
        listeners.add(l);
        return () => void listeners.delete(l);
      }, []);
      return value;
    },
  };
}

// ---------------------------------------------------------------------------------------------
// Context menu
// ---------------------------------------------------------------------------------------------

export interface MenuItem {
  label?: string;
  action?: () => void;
  disabled?: boolean;
  danger?: boolean;
  separator?: boolean;
  hint?: string;
  submenu?: MenuItem[];
}

const menuState = observable<{ x: number; y: number; items: MenuItem[] } | null>(null);

export function showMenu(e: { clientX: number; clientY: number; preventDefault?: () => void; stopPropagation?: () => void }, items: MenuItem[]) {
  e.preventDefault?.();
  e.stopPropagation?.();
  // Drop leading/trailing/double separators produced by conditional item lists.
  const clean: MenuItem[] = [];
  for (const it of items) {
    if (it.separator && (clean.length === 0 || clean[clean.length - 1].separator)) continue;
    clean.push(it);
  }
  while (clean.length && clean[clean.length - 1].separator) clean.pop();
  if (clean.length) menuState.set({ x: e.clientX, y: e.clientY, items: clean });
}

export const sep: MenuItem = { separator: true };

function MenuList({ items, x, y, onDone }: { items: MenuItem[]; x: number; y: number; onDone: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [open, setOpen] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    let nx = x, ny = y;
    if (nx + r.width > window.innerWidth - 4) nx = Math.max(4, window.innerWidth - r.width - 4);
    if (ny + r.height > window.innerHeight - 4) ny = Math.max(4, window.innerHeight - r.height - 4);
    setPos({ x: nx, y: ny });
  }, [x, y]);
  return (
    <div class="menu" ref={ref} style={{ left: px(pos.x), top: px(pos.y) }} onMouseDown={(e) => e.stopPropagation()}>
      {items.map((it, i) =>
        it.separator ? (
          <div key={i} class="menu-sep" />
        ) : (
          <div
            key={i}
            class={`menu-item${it.disabled ? " disabled" : ""}${it.danger ? " danger" : ""}${open === i ? " open" : ""}`}
            onMouseEnter={() => setOpen(it.submenu ? i : null)}
            onClick={() => {
              if (it.disabled || it.submenu) return;
              onDone();
              it.action?.();
            }}
          >
            <span class="menu-label">{it.label}</span>
            {it.hint && <span class="menu-hint">{it.hint}</span>}
            {it.submenu && <span class="menu-arrow">▸</span>}
            {it.submenu && open === i && (
              <SubMenu items={it.submenu} parent={ref} index={i} onDone={onDone} />
            )}
          </div>
        ),
      )}
    </div>
  );
}

function SubMenu({ items, parent, index, onDone }: { items: MenuItem[]; parent: { current: HTMLDivElement | null }; index: number; onDone: () => void }) {
  const el = parent.current?.children[index] as HTMLElement | undefined;
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return <MenuList items={items} x={r.right - 2} y={r.top - 4} onDone={onDone} />;
}

function MenuHost() {
  const m = menuState.use();
  useEffect(() => {
    if (!m) return;
    const close = () => menuState.set(null);
    const key = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("keydown", key);
    };
  }, [m]);
  if (!m) return null;
  return <MenuList items={m.items} x={m.x} y={m.y} onDone={() => menuState.set(null)} />;
}

// ---------------------------------------------------------------------------------------------
// Form dialogs
// ---------------------------------------------------------------------------------------------

export interface Field {
  name: string;
  label: string;
  type: "text" | "checkbox" | "select" | "textarea";
  value?: string | boolean;
  options?: { value: string; label: string }[];
  placeholder?: string;
  hint?: string;
}

export type FormValues = Record<string, any>;

interface DialogSpec {
  title: string;
  message?: ComponentChildren;
  fields: Field[];
  ok: string;
  danger?: boolean;
  validate?: (v: FormValues) => string | null;
  resolve: (v: FormValues | null) => void;
}

const dialogState = observable<DialogSpec | null>(null);

export function form(opts: {
  title: string;
  message?: ComponentChildren;
  fields?: Field[];
  ok?: string;
  danger?: boolean;
  validate?: (v: FormValues) => string | null;
}): Promise<FormValues | null> {
  return new Promise((resolve) => {
    dialogState.set({ fields: [], ok: "OK", ...opts, resolve });
  });
}

export async function confirm(title: string, message: ComponentChildren, ok = "OK", danger = false) {
  return (await form({ title, message, ok, danger })) !== null;
}

function DialogHost() {
  const d = dialogState.use();
  const [values, setValues] = useState<FormValues>({});
  const [error, setError] = useState<string | null>(null);
  const firstRef = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (!d) return;
    const v: FormValues = {};
    d.fields.forEach((f) => (v[f.name] = f.value ?? (f.type === "checkbox" ? false : f.type === "select" ? f.options?.[0]?.value ?? "" : "")));
    setValues(v);
    setError(null);
    setTimeout(() => {
      const el = firstRef.current as HTMLInputElement | null;
      el?.focus();
      el?.select?.();
    }, 0);
  }, [d]);
  if (!d) return null;
  const close = (v: FormValues | null) => {
    dialogState.set(null);
    d.resolve(v);
  };
  const submit = () => {
    const err = d.validate?.(values) ?? null;
    if (err) return setError(err);
    close(values);
  };
  let first = true;
  return (
    <div class="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close(null)}>
      <div
        class="dialog"
        onKeyDown={(e) => {
          if (e.key === "Escape") close(null);
          if (e.key === "Enter" && (e.target as HTMLElement).tagName !== "TEXTAREA") {
            e.preventDefault();
            submit();
          }
        }}
      >
        <div class="dialog-title">{d.title}</div>
        {d.message && <div class="dialog-message">{d.message}</div>}
        {d.fields.map((f) => {
          const setRef = first ? (el: HTMLElement | null) => (firstRef.current = el) : undefined;
          first = false;
          const set = (val: any) => setValues((v) => ({ ...v, [f.name]: val }));
          if (f.type === "checkbox")
            return (
              <label class="field-check" key={f.name}>
                <input ref={setRef as any} type="checkbox" checked={!!values[f.name]} onChange={(e) => set((e.target as HTMLInputElement).checked)} />
                {f.label}
                {f.hint && <span class="field-hint"> — {f.hint}</span>}
              </label>
            );
          return (
            <label class="field" key={f.name}>
              <span class="field-label">{f.label}</span>
              {f.type === "select" ? (
                <select ref={setRef as any} value={values[f.name]} onChange={(e) => set((e.target as HTMLSelectElement).value)}>
                  {f.options?.map((o) => (
                    <option value={o.value}>{o.label}</option>
                  ))}
                </select>
              ) : f.type === "textarea" ? (
                <textarea ref={setRef as any} rows={4} placeholder={f.placeholder} value={values[f.name]} onInput={(e) => set((e.target as HTMLTextAreaElement).value)} />
              ) : (
                <input ref={setRef as any} type="text" spellcheck={false} placeholder={f.placeholder} value={values[f.name]} onInput={(e) => set((e.target as HTMLInputElement).value)} />
              )}
              {f.hint && <span class="field-hint">{f.hint}</span>}
            </label>
          );
        })}
        {error && <div class="dialog-error">{error}</div>}
        <div class="dialog-buttons">
          <button onClick={() => close(null)}>Cancel</button>
          <button class={d.danger ? "danger" : "primary"} onClick={submit} ref={d.fields.length ? undefined : (el) => void (el && setTimeout(() => el.focus(), 0))}>
            {d.ok}
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------------------------

interface Toast {
  id: number;
  text: string;
  kind: "info" | "error";
}
const toastState = observable<Toast[]>([]);
let toastId = 0;

export function toast(text: string, kind: "info" | "error" = "info") {
  const id = ++toastId;
  toastState.set([...toastState.get(), { id, text, kind }]);
  setTimeout(() => toastState.set(toastState.get().filter((t) => t.id !== id)), kind === "error" ? 8000 : 3500);
}

export function toastError(e: unknown) {
  toast(String((e as any)?.message ?? e), "error");
}

function ToastHost() {
  const list = toastState.use();
  return (
    <div class="toasts">
      {list.map((t) => (
        <div key={t.id} class={`toast ${t.kind}`} onClick={() => toastState.set(toastState.get().filter((x) => x.id !== t.id))}>
          {t.text}
        </div>
      ))}
    </div>
  );
}

export function UiHost() {
  return (
    <>
      <DialogHost />
      <MenuHost />
      <ToastHost />
    </>
  );
}
