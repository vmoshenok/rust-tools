// Drag handle; reports the total delta (pixels) since the drag started, so callers can
// compute `sizeAtDragStart + delta` from the values captured at mousedown.
export function Splitter({ dir, onDrag }: { dir: "x" | "y"; onDrag: (delta: number) => void }) {
  const down = (e: MouseEvent) => {
    e.preventDefault();
    const start = dir === "x" ? e.clientX : e.clientY;
    const move = (m: MouseEvent) => onDrag((dir === "x" ? m.clientX : m.clientY) - start);
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.classList.remove("dragging-x", "dragging-y");
    };
    document.body.classList.add(dir === "x" ? "dragging-x" : "dragging-y");
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  return <div class={`splitter splitter-${dir}`} onMouseDown={down} />;
}
