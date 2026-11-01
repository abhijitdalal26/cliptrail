// Simplified TypeScript twin of crates/clipd-core/src/transform.rs, used only
// by the browser mock (the real app calls the Rust implementation).

import type { Kind, Transform } from "./types";

const TRACKING = new Set([
  "fbclid", "gclid", "gclsrc", "gbraid", "wbraid", "dclid", "msclkid", "mc_cid", "mc_eid", "igshid",
  "ref_src", "spm", "_hsenc", "_hsmi", "yclid", "twclid", "ttclid", "mkt_tok", "_ga", "_gl",
]);

function isTracking(key: string, host: string): boolean {
  const k = key.toLowerCase();
  if (k.startsWith("utm_") || k.startsWith("wt_mc") || TRACKING.has(k)) return true;
  const on = (d: string) => host === d || host.endsWith(`.${d}`);
  if (k === "si") return on("youtube.com") || on("youtu.be") || on("spotify.com");
  return false;
}

function cleanUrl(text: string): string {
  const t = text.trim();
  const u = new URL(t);
  const hashAt = t.indexOf("#");
  const rest = hashAt >= 0 ? t.slice(0, hashAt) : t;
  const frag = hashAt >= 0 ? t.slice(hashAt) : "";
  const q = rest.indexOf("?");
  if (q < 0) return t;
  const segs = rest.slice(q + 1).split("&");
  const kept = segs.filter((s) => !isTracking(decodeURIComponentSafe(s.split("=")[0]), u.hostname.toLowerCase()));
  if (kept.length === segs.length) return t;
  return rest.slice(0, q) + (kept.length ? `?${kept.join("&")}` : "") + frag;
}

function decodeURIComponentSafe(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

// ---- colors
interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

function parseColor(t: string): { c: Rgba; fmt: "hex" | "rgb" | "hsl" } | null {
  const s = t.trim().toLowerCase();
  let m = s.match(/^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/);
  if (m) {
    let h = m[1];
    if (h.length <= 4) h = [...h].map((x) => x + x).join("");
    const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
    return { c: { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? n(6) / 255 : 1 }, fmt: "hex" };
  }
  m = s.match(/^(rgba?|hsla?)\(([^)]+)\)$/);
  if (!m) return null;
  const parts = m[2].split(/[\s,/]+/).filter(Boolean);
  if (parts.length < 3 || parts.length > 4) return null;
  const num = (p: string) => parseFloat(p);
  const a = parts[3] ? (parts[3].endsWith("%") ? num(parts[3]) / 100 : num(parts[3])) : 1;
  if (parts.some((p) => Number.isNaN(num(p)))) return null;
  if (m[1].startsWith("rgb")) {
    const ch = (p: string) => Math.max(0, Math.min(255, Math.round(p.endsWith("%") ? num(p) * 2.55 : num(p))));
    return { c: { r: ch(parts[0]), g: ch(parts[1]), b: ch(parts[2]), a }, fmt: "rgb" };
  }
  const h = (((num(parts[0]) % 360) + 360) % 360) / 360;
  const sat = num(parts[1]) / 100;
  const l = num(parts[2]) / 100;
  const q = l < 0.5 ? l * (1 + sat) : l + sat - l * sat;
  const p = 2 * l - q;
  const f = (tt: number) => {
    const x = (tt + 1) % 1;
    const v = x < 1 / 6 ? p + (q - p) * 6 * x : x < 1 / 2 ? q : x < 2 / 3 ? p + (q - p) * (2 / 3 - x) * 6 : p;
    return Math.round(v * 255);
  };
  return { c: { r: f(h + 1 / 3), g: f(h), b: f(h - 1 / 3), a }, fmt: "hsl" };
}

const trimNum = (x: number, d: number) => String(Number(x.toFixed(d)));

function colorTo(text: string, to: "hex" | "rgb" | "hsl"): string {
  const p = parseColor(text);
  if (!p) throw new Error("not a color");
  const { r, g, b } = p.c;
  const a = Math.round(p.c.a * 1000) / 1000;
  if (to === "hex") {
    const hex = (n: number) => n.toString(16).padStart(2, "0");
    let s = `#${hex(r)}${hex(g)}${hex(b)}`;
    if (a < 1) s += hex(Math.round(a * 255));
    return /^#.*[A-F]/.test(text.trim()) ? s.toUpperCase() : s;
  }
  if (to === "rgb") return a < 1 ? `rgba(${r}, ${g}, ${b}, ${trimNum(a, 3)})` : `rgb(${r}, ${g}, ${b})`;
  const rn = r / 255, gn = g / 255, bn = b / 255;
  const max = Math.max(rn, gn, bn), min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  const d = max - min;
  let h = 0, s = 0;
  if (d !== 0) {
    s = d / (1 - Math.abs(2 * l - 1));
    h = max === rn ? (((gn - bn) / d) % 6 + 6) % 6 : max === gn ? (bn - rn) / d + 2 : (rn - gn) / d + 4;
    h *= 60;
  }
  const hs = `${trimNum(h, 1)}, ${trimNum(s * 100, 1)}%, ${trimNum(l * 100, 1)}%`;
  return a < 1 ? `hsla(${hs}, ${trimNum(a, 3)})` : `hsl(${hs})`;
}

// ---- the rest
function dedent(text: string): string {
  const lines = text.split("\n");
  let common: string | null = null;
  for (const l of lines) {
    if (!l.trim()) continue;
    const ind = l.slice(0, l.length - l.trimStart().length);
    if (common === null) common = ind;
    else {
      let i = 0;
      while (i < common.length && i < ind.length && common[i] === ind[i]) i++;
      common = common.slice(0, i);
    }
  }
  const pre = common ?? "";
  return lines.map((l) => (l.trim() ? l.slice(pre.length) : "")).join("\n");
}

export function applyMock(t: Transform, text: string, sub: string | null): string {
  switch (t) {
    case "plain-text":
      return text;
    case "trim":
      return text.trim().split("\n").map((l) => l.trimEnd()).join("\n");
    case "upper-case":
      return text.toUpperCase();
    case "lower-case":
      return text.toLowerCase();
    case "title-case":
      return text.replace(/[\p{L}\p{N}']+/gu, (w) => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase());
    case "single-line":
      return text.split("\n").map((l) => l.trim()).filter(Boolean).join(" ");
    case "clean-url":
      return cleanUrl(text);
    case "markdown-link": {
      const u = new URL(text.trim());
      const label = (u.hostname.replace(/^www\./, "") + u.pathname).replace(/\/$/, "");
      return `[${label}](${text.trim().replace(/\(/g, "%28").replace(/\)/g, "%29")})`;
    }
    case "url-decode":
      return decodeURIComponent(text.trim());
    case "color-hex":
      return colorTo(text, "hex");
    case "color-rgb":
      return colorTo(text, "rgb");
    case "color-hsl":
      return colorTo(text, "hsl");
    case "json-pretty":
      return JSON.stringify(JSON.parse(text), null, 2);
    case "json-minify":
      return JSON.stringify(JSON.parse(text));
    case "markdown-code-block": {
      const body = text.replace(/\n+$/, "");
      const fence = "`".repeat(Math.max(3, ...(body.match(/`+/g) ?? []).map((r) => r.length + 1)));
      return `${fence}${(sub ?? "").replace(/[^\w+#.-]/g, "")}\n${body}\n${fence}`;
    }
    case "dedent":
      return dedent(text);
    case "phone-digits": {
      const d = text.trim().replace(/\D/g, "");
      if (!d) throw new Error("no digits");
      return (text.trim().startsWith("+") ? "+" : "") + d;
    }
    case "mailto-link":
      if (!text.includes("@")) throw new Error("not an email address");
      return text.trim().toLowerCase().startsWith("mailto:") ? text.trim() : `mailto:${text.trim()}`;
  }
}

export function availableMock(kind: Kind, sub: string | null, text: string): Transform[] {
  const t = text.trim();
  if (!t) return [];
  const multi = t.includes("\n");
  let c: Transform[];
  switch (kind) {
    case "link":
      c = ["clean-url", "markdown-link", "url-decode"];
      break;
    case "color":
      c = ["color-hex", "color-rgb", "color-hsl"];
      break;
    case "code":
      c = [
        ...((sub === "json" || /^[{[]/.test(t) ? ["json-pretty", "json-minify"] : []) as Transform[]),
        ...((t.startsWith("```") ? [] : ["markdown-code-block"]) as Transform[]),
        ...((multi ? ["dedent"] : []) as Transform[]),
        "trim",
      ];
      break;
    case "contact":
      c = sub === "email" ? ["mailto-link"] : ["phone-digits"];
      break;
    case "text":
      c = ["trim", "single-line", ...((multi ? ["dedent"] : []) as Transform[]), "upper-case", "lower-case", "title-case"];
      break;
    default:
      return [];
  }
  const own = kind === "color" ? parseColor(t)?.fmt : null;
  return c.filter((x) => {
    if (own && ((x === "color-rgb" && own === "rgb") || (x === "color-hsl" && own === "hsl"))) return false;
    try {
      const out = applyMock(x, text, sub);
      return x === "json-pretty" || x === "json-minify" ? out.trimEnd() !== text.trimEnd() : out !== text;
    } catch {
      return false;
    }
  });
}
