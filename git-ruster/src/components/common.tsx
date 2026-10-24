// Small shared presentational helpers.
import { px } from "../lib/css";

const AVATAR_COLORS = ["#c0504d", "#4f81bd", "#9bbb59", "#8064a2", "#4bacc6", "#f79646", "#2c8c7c", "#b0558c", "#6b7fd7", "#a68b2d"];

export function Avatar({ name, size = 20 }: { name: string; size?: number }) {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  const initials = name
    .split(/[\s._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
  return (
    <span class="avatar" style={{ width: px(size), height: px(size), fontSize: px(size * 0.45), background: AVATAR_COLORS[Math.abs(h) % AVATAR_COLORS.length] }}>
      {initials || "?"}
    </span>
  );
}

export function formatDate(unix: number, long = false) {
  if (!unix) return "";
  const d = new Date(unix * 1000);
  const now = new Date();
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (long) return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  const days = Math.floor((new Date(now.toDateString()).getTime() - new Date(d.toDateString()).getTime()) / 86400000);
  if (days === 0) return `Today ${time}`;
  if (days === 1) return `Yesterday ${time}`;
  return d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }) + " " + time;
}

/** Status letter → css class + label. */
export function statusInfo(letter: string): { cls: string; label: string } {
  switch (letter) {
    case "A":
      return { cls: "st-added", label: "Added" };
    case "D":
      return { cls: "st-deleted", label: "Deleted" };
    case "R":
      return { cls: "st-renamed", label: "Renamed" };
    case "C":
      return { cls: "st-renamed", label: "Copied" };
    case "?":
      return { cls: "st-untracked", label: "Unversioned" };
    case "U":
      return { cls: "st-conflict", label: "Conflict" };
    case "T":
      return { cls: "st-modified", label: "Type changed" };
    default:
      return { cls: "st-modified", label: "Modified" };
  }
}

export function splitPath(p: string): { name: string; dir: string } {
  const i = p.lastIndexOf("/");
  return i < 0 ? { name: p, dir: "" } : { name: p.slice(i + 1), dir: p.slice(0, i) };
}
