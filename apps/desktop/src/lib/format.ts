import {
  differenceInCalendarDays,
  format,
  formatDistanceToNowStrict,
  isSameYear,
  isToday,
  isYesterday,
} from "date-fns";

export function dayKey(ms: number): string {
  return format(ms, "yyyy-MM-dd");
}

export function dayLabel(ms: number): string {
  if (isToday(ms)) return "Today";
  if (isYesterday(ms)) return "Yesterday";
  const days = differenceInCalendarDays(Date.now(), ms);
  if (days < 7) return format(ms, "EEEE");
  return isSameYear(ms, Date.now()) ? format(ms, "EEE, MMM d") : format(ms, "EEE, MMM d, yyyy");
}

/** "now", "4m", "3h", or a clock time / date for older items. */
export function shortTime(ms: number): string {
  const diff = Date.now() - ms;
  if (diff < 45_000) return "now";
  if (diff < 3_600_000) return `${Math.max(1, Math.round(diff / 60_000))}m`;
  if (diff < 6 * 3_600_000) return `${Math.round(diff / 3_600_000)}h`;
  if (isToday(ms) || differenceInCalendarDays(Date.now(), ms) < 7) return format(ms, "h:mm a");
  return format(ms, "MMM d");
}

export function relative(ms: number): string {
  const diff = Date.now() - ms;
  if (diff < 45_000) return "just now";
  return `${formatDistanceToNowStrict(ms)} ago`;
}

export function fullDate(ms: number): string {
  return format(ms, "EEE, MMM d, yyyy 'at' h:mm a");
}

export function untilLabel(ms: number): string {
  const diff = ms - Date.now();
  if (diff <= 0) return "expiring";
  if (diff < 3_600_000) return `${Math.max(1, Math.round(diff / 60_000))} min`;
  if (diff < 48 * 3_600_000) return `${Math.round(diff / 3_600_000)} h`;
  return `${Math.round(diff / 86_400_000)} days`;
}

export function compact(n: number): string {
  if (n < 1000) return String(n);
  if (n < 10_000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  return `${Math.round(n / 1000)}k`;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

export interface ParsedUrl {
  host: string;
  path: string;
  href: string;
}

export function parseUrl(text: string): ParsedUrl | null {
  try {
    const u = new URL(text.trim());
    const path = decodeURI(u.pathname + u.search + u.hash).replace(/\/$/, "");
    return { host: u.hostname.replace(/^www\./, ""), path, href: u.href };
  } catch {
    return null;
  }
}

export function splitPath(p: string): { name: string; dir: string } {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  if (i < 0) return { name: p, dir: "" };
  return { name: p.slice(i + 1) || p, dir: p.slice(0, i) };
}

/** Parse a CSS color literal into rgb + a normalized hex. */
export function parseColor(text: string): { css: string; hex: string; rgb: string; hsl: string } | null {
  const t = text.trim();
  const el = document.createElement("div");
  el.style.color = "";
  el.style.color = t;
  if (!el.style.color) return null;
  // Resolve through canvas to get rgb numbers.
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return null;
  ctx.fillStyle = "#000";
  ctx.fillStyle = t;
  const v = ctx.fillStyle as string; // "#rrggbb" or "rgba(...)"
  let r = 0,
    g = 0,
    b = 0,
    a = 1;
  if (v.startsWith("#")) {
    r = parseInt(v.slice(1, 3), 16);
    g = parseInt(v.slice(3, 5), 16);
    b = parseInt(v.slice(5, 7), 16);
  } else {
    const m = v.match(/[\d.]+/g);
    if (!m) return null;
    [r, g, b] = m.slice(0, 3).map(Number);
    a = m[3] != null ? Number(m[3]) : 1;
  }
  const hex = `#${[r, g, b].map((x) => x.toString(16).padStart(2, "0")).join("")}`.toUpperCase();
  const rn = r / 255,
    gn = g / 255,
    bn = b / 255;
  const max = Math.max(rn, gn, bn),
    min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  let h = 0,
    s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    h =
      max === rn ? (gn - bn) / d + (gn < bn ? 6 : 0) : max === gn ? (bn - rn) / d + 2 : (rn - gn) / d + 4;
    h *= 60;
  }
  const alpha = a < 1 ? ` / ${Math.round(a * 100)}%` : "";
  return {
    css: t,
    hex,
    rgb: `rgb(${r} ${g} ${b}${alpha})`,
    hsl: `hsl(${Math.round(h)} ${Math.round(s * 100)}% ${Math.round(l * 100)}%${alpha})`,
  };
}

/** Relative luminance, to pick readable text over a swatch. */
export function isLightColor(hex: string): boolean {
  const n = parseInt(hex.slice(1), 16);
  const r = (n >> 16) & 255,
    g = (n >> 8) & 255,
    b = n & 255;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 160;
}
