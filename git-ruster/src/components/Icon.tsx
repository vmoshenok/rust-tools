// Minimal stroke icon set (24x24 viewBox).
const paths: Record<string, string> = {
  fetch: "M12 3v12m0 0l-4-4m4 4l4-4M4 19h16",
  pull: "M12 4v12m0 0l-5-5m5 5l5-5M5 20h14",
  push: "M12 20V8m0 0l-5 5m5-5l5 5M5 4h14",
  branch: "M6 3v12M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a9 9 0 0 1-9 9",
  stash: "M4 7h16M6 7v11a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7M9 4h6M10 11h4",
  pop: "M4 17h16M6 17V6a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v11M12 14V8m0 0l-3 3m3-3l3 3",
  terminal: "M4 5h16v14H4zM7 9l3 3-3 3M12 15h5",
  refresh: "M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7",
  folder: "M3 6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z",
  repo: "M5 4h11a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3zM5 17a3 3 0 0 1 3-3h11",
  settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  plus: "M12 5v14M5 12h14",
  commit: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM3 12h6M15 12h6",
  search: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM21 21l-4.3-4.3",
  down: "M6 9l6 6 6-6",
  right: "M9 6l6 6-6 6",
  left: "M15 6l-6 6 6 6",
  up: "M6 15l6-6 6 6",
  x: "M6 6l12 12M18 6L6 18",
  check: "M5 12l5 5 9-10",
  tag: "M3 12V4a1 1 0 0 1 1-1h8l9 9-9 9zM8 8h.01",
  cloud: "M7 18a4 4 0 0 1-.5-8A6 6 0 0 1 18 9a4.5 4.5 0 0 1-.5 9z",
  undo: "M9 14L4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3",
  merge: "M6 3v18M6 9a6 6 0 0 0 6 6h3M18 18a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
  warning: "M12 3l9 16H3zM12 10v4M12 17h.01",
  diff: "M12 3v18M5 8h4M7 6v4M15 15h4",
  swap: "M7 4L3 8l4 4M3 8h14M17 20l4-4-4-4M21 16H7",
  filter: "M3 5h18l-7 8v6l-4 2v-8z",
  explorer: "M14 3h7v7M21 3l-9 9M19 14v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5",
};

export function Icon({ name, size = 16, class: cls }: { name: keyof typeof paths | string; size?: number; class?: string }) {
  return (
    <svg
      class={`icon ${cls ?? ""}`}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <path d={paths[name] ?? ""} />
    </svg>
  );
}
