// In-memory backend used when the UI runs outside Tauri (`pnpm dev` in a
// browser). Serves ~150 realistic clips of every kind over ~60 days.

import { applyMock, availableMock } from "./mockTransforms";
import type { CaptureState, Clip, DayCount, Kind, ListQuery, Settings, SettingsView, Stats, Transform } from "./types";

// ---------- deterministic randomness ----------
let seed = 20261101;
function rnd(): number {
  seed = (seed * 1664525 + 1013904223) % 4294967296;
  return seed / 4294967296;
}
const pick = <T,>(xs: T[]): T => xs[Math.floor(rnd() * xs.length)];

// ---------- content pools ----------
const LINKS: [string, string][] = [
  ["https://github.com/tauri-apps/tauri/pull/11842", "chrome.exe"],
  ["https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html", "chrome.exe"],
  ["https://linear.app/cliptrail/issue/CT-142/picker-should-remember-last-kind-filter", "Linear.exe"],
  ["https://www.figma.com/design/Qk2pX7cLw1/ClipTrail-Desktop?node-id=214-3120", "Figma.exe"],
  ["https://news.ycombinator.com/item?id=41837265", "firefox.exe"],
  ["https://developer.mozilla.org/en-US/docs/Web/CSS/backdrop-filter", "msedge.exe"],
  ["https://tailwindcss.com/docs/theme#using-css-variables", "chrome.exe"],
  ["https://www.youtube.com/watch?v=Mf2EbJZgBvY&si=Zk3Lw9QpXa2_Tq1B", "chrome.exe"],
  ["https://en.wikipedia.org/wiki/Bloom_filter", "firefox.exe"],
  ["https://stackoverflow.com/questions/63451478/how-to-use-sqlite-fts5-trigram-tokenizer", "chrome.exe"],
  ["https://www.sqlite.org/fts5.html#the_trigram_tokenizer", "chrome.exe"],
  ["https://maps.google.com/?q=Blue+Tokai+Coffee+Bandra", "chrome.exe"],
  ["https://vercel.com/blog/how-we-made-the-dashboard-twice-as-fast?utm_source=newsletter&utm_medium=email&utm_campaign=oct&ref=home#benchmarks", "msedge.exe"],
  ["https://www.notion.so/cliptrail/Launch-checklist-9a7c51e0d2b04f7e", "Notion.exe"],
  ["https://crates.io/crates/window-vibrancy", "firefox.exe"],
  ["https://arxiv.org/abs/2310.06825", "chrome.exe"],
  ["https://open.spotify.com/playlist/37i9dQZF1DX8Uebhn9wzrS?si=a1b2c3d4e5f60718", "Spotify.exe"],
  ["https://react.dev/reference/react/useDeferredValue", "chrome.exe"],
  ["https://tanstack.com/virtual/latest/docs/api/virtualizer", "chrome.exe"],
  ["https://www.rust-lang.org/learn", "firefox.exe"],
];

const CODE: [string, string, string][] = [
  [
    "rust",
    `pub fn ingest(&self, raw: RawClip) -> Result<IngestOutcome> {
    let normalized = normalize(&raw.content)?;
    let hash = Sha256::digest(normalized.as_bytes()).to_vec();

    if let Some(app) = raw.source_app.as_deref() {
        if self.config.excluded_apps.iter().any(|a| a.eq_ignore_ascii_case(app)) {
            return Ok(IngestOutcome::Skipped { reason: "excluded app".into() });
        }
    }

    let class = classify_text(&normalized);
    let (masked, expires_at) = match class.kind {
        Kind::Sensitive => (true, Some(raw.captured_at + self.config.sensitive_ttl_ms)),
        _ => (false, None),
    };

    let clip = NewClip {
        id: Ulid::new().to_string(),
        content_hash: hash,
        kind: class.kind,
        subkind: class.subkind,
        text: Some(normalized),
        masked,
        expires_at,
        ..NewClip::from_raw(&raw, &self.config.device_id)
    };

    let (id, inserted) = self.store.upsert(clip)?;
    tracing::debug!(%id, kind = class.kind.as_str(), inserted, "ingested");
    Ok(if inserted {
        IngestOutcome::Stored { id, kind: class.kind }
    } else {
        IngestOutcome::Bumped { id, kind: class.kind }
    })
}`,
    "Code.exe",
  ],
  [
    "typescript",
    `import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";

export function Timeline({ rows }: { rows: Row[] }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (i) => (rows[i].type === "header" ? 36 : 76),
    overscan: 12,
  });

  return (
    <div ref={parentRef} className="h-full overflow-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((item) => (
          <div
            key={item.key}
            data-index={item.index}
            ref={virtualizer.measureElement}
            style={{ position: "absolute", top: 0, transform: \`translateY(\${item.start}px)\` }}
          >
            <RowView row={rows[item.index]} />
          </div>
        ))}
      </div>
    </div>
  );
}`,
    "Code.exe",
  ],
  [
    "python",
    `import sqlite3
from pathlib import Path

def top_domains(db: Path, days: int = 7) -> list[tuple[str, int]]:
    """Most-copied link hosts over the last N days."""
    con = sqlite3.connect(db)
    rows = con.execute(
        """
        SELECT subkind, COUNT(*) AS n
        FROM clips
        WHERE kind = 'link'
          AND last_seen_at >= strftime('%s', 'now', ?) * 1000
        GROUP BY subkind
        ORDER BY n DESC
        LIMIT 10
        """,
        (f"-{days} days",),
    ).fetchall()
    con.close()
    return rows`,
    "WindowsTerminal.exe",
  ],
  [
    "sql",
    `CREATE VIRTUAL TABLE clips_fts USING fts5(
  body,
  content='clips',
  content_rowid='rowid',
  tokenize='trigram'
);`,
    "Code.exe",
  ],
  ["shell", `pnpm tauri build --debug && ./target/debug/cliptrail.exe`, "WindowsTerminal.exe"],
  ["shell", `git log --oneline --graph --decorate -n 20`, "WindowsTerminal.exe"],
  ["shell", `docker run -d -p 8080:8080 -v cliptrail:/data ghcr.io/cliptrail/relay:latest`, "WindowsTerminal.exe"],
  [
    "json",
    `{
  "productName": "ClipTrail",
  "identifier": "dev.cliptrail.desktop",
  "app": {
    "security": {
      "assetProtocol": { "enable": true, "scope": [] }
    }
  },
  "bundle": { "targets": ["nsis", "msi", "deb", "appimage"] }
}`,
    "Code.exe",
  ],
  [
    "css",
    `.card {
  border: 1px solid var(--border);
  border-radius: 12px;
  background: color-mix(in oklab, var(--surface) 92%, transparent);
  box-shadow: 0 1px 2px rgb(0 0 0 / 0.04), 0 8px 24px -12px rgb(0 0 0 / 0.12);
  transition: background-color 160ms ease, border-color 160ms ease;
}`,
    "Code.exe",
  ],
  [
    "go",
    `func (s *Server) handleSync(w http.ResponseWriter, r *http.Request) {
	conn, err := s.upgrader.Upgrade(w, r, nil)
	if err != nil {
		http.Error(w, "upgrade failed", http.StatusBadRequest)
		return
	}
	defer conn.Close()
	for {
		_, msg, err := conn.ReadMessage()
		if err != nil {
			return
		}
		s.broadcast(r.Context(), msg)
	}
}`,
    "Code.exe",
  ],
  [
    "yaml",
    `name: ci
on: [push, pull_request]
jobs:
  test:
    runs-on: \${{ matrix.os }}
    strategy:
      matrix:
        os: [windows-latest, ubuntu-latest]
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --workspace`,
    "Code.exe",
  ],
  ["rust", `let data_dir = clipd_core::default_data_dir();`, "Code.exe"],
  ["typescript", `const debounced = useDeferredValue(query.trim());`, "Code.exe"],
  [
    "kotlin",
    `class ClipTileService : TileService() {
    override fun onClick() {
        val clip = clipboard.primaryClip ?: return
        ClipRepository.save(clip, source = "tile")
        qsTile.apply { state = Tile.STATE_ACTIVE; updateTile() }
    }
}`,
    "studio64.exe",
  ],
];

const TEXT: [string, string][] = [
  [
    `The trick with clipboard history isn't capture — every OS hands you the change events. It's recall. Six weeks from now you'll remember that you copied "something about trigram tokenizers" on a Tuesday, not the exact string. So the timeline needs to be fast to scan by day, the search has to match substrings rather than whole words, and the cards need to look different enough per kind that your eye can skip the noise. Everything else is secondary.

The second thing is trust. People copy passwords, recovery codes and private messages. The default has to be local-only, encrypted at rest, with sensitive clips masked and expiring on their own.`,
    "Obsidian.exe",
  ],
  [
    `Meeting notes — design review, Thu
- Picker: keep it at 640px, chips scroll horizontally if they overflow
- Dashboard: preview pane on the right, 420px min
- Heatmap: 16 weeks, click to jump
- Ship dark theme first, light needs another pass on borders
- Open question: should pinned clips float to the top of every view?`,
    "Notion.exe",
  ],
  ["Thanks so much — this works perfectly now. I'll send the updated deck tonight.", "Slack.exe"],
  ["Let's push the launch to Thursday so QA has the full day on Wednesday.", "Slack.exe"],
  ["ClipTrail keeps everything you copy — searchable, by day, on every device.", "Figma.exe"],
  ["14th Floor, One BKC, Bandra Kurla Complex, Mumbai 400051", "chrome.exe"],
  ["Ship it 🚀", "Discord.exe"],
  [
    `Dear Ms. Rao,

Thank you for taking the time to meet with us last week. As discussed, I've attached the revised proposal reflecting the updated scope: the desktop client for Windows and Linux in the first phase, followed by Android. The timeline assumes two weeks for the capture layer, one week for search and the dashboard, and a final week of polish and packaging.

Please let me know if you'd like to walk through the estimates together. I'm free Tuesday or Wednesday afternoon.

Best regards,
Abhijit`,
    "OUTLOOK.EXE",
  ],
  ["fts5 trigram tokenizer case sensitive", "chrome.exe"],
  ["Q3 revenue grew 18% quarter over quarter, driven mostly by the self-serve tier.", "EXCEL.EXE"],
  [
    `Groceries
- oat milk
- eggs (12)
- sourdough
- spinach
- coffee beans — light roast`,
    "Notepad.exe",
  ],
  ["The quick brown fox jumps over the lazy dog", "WINWORD.EXE"],
  ["Paste-back needs the previous foreground window restored before SendInput fires Ctrl+V.", "Obsidian.exe"],
  ["Out of office until Monday — reach Priya for anything urgent.", "Teams.exe"],
  ["git commit -m \"fix: debounce search\" was rejected by the pre-commit hook", "Slack.exe"],
  [
    `It was the best of times, it was the worst of times, it was the age of wisdom, it was the age of foolishness, it was the epoch of belief, it was the epoch of incredulity, it was the season of Light, it was the season of Darkness, it was the spring of hope, it was the winter of despair, we had everything before us, we had nothing before us, we were all going direct to Heaven, we were all going direct the other way — in short, the period was so far like the present period, that some of its noisiest authorities insisted on its being received, for good or for evil, in the superlative degree of comparison only.`,
    "firefox.exe",
  ],
];

const CONTACTS: [string, string, string][] = [
  ["priya.sharma@acme.io", "email", "OUTLOOK.EXE"],
  ["hello@cliptrail.dev", "email", "chrome.exe"],
  ["+91 98201 44718", "phone", "WhatsApp.exe"],
  ["+1 (415) 555-0132", "phone", "chrome.exe"],
  ["jonas.weber@studio-nord.de", "email", "Slack.exe"],
];

const COLORS: [string, string][] = [
  ["#6E56CF", "hex"],
  ["#0EA5E9", "hex"],
  ["rgb(34, 197, 94)", "rgb"],
  ["#F97316", "hex"],
  ["hsl(346, 77%, 50%)", "hsl"],
  ["#111113", "hex"],
  ["#FAFAFA", "hex"],
  ["#EAB308", "hex"],
];

const FILES: string[][] = [
  ["C:\\Users\\abhijit\\Documents\\Proposals\\cliptrail-proposal-v3.pdf"],
  ["C:\\Users\\abhijit\\Pictures\\Screenshots\\Screenshot 2026-10-28 141502.png", "C:\\Users\\abhijit\\Pictures\\Screenshots\\Screenshot 2026-10-28 141533.png", "C:\\Users\\abhijit\\Pictures\\Screenshots\\Screenshot 2026-10-28 141610.png"],
  ["C:\\Users\\abhijit\\Desktop\\cliptrail\\docs\\system-design.md"],
  ["D:\\Design\\Brand\\logo-mark.svg", "D:\\Design\\Brand\\logo-wordmark.svg"],
  ["C:\\Users\\abhijit\\Downloads\\invoice-2026-10.xlsx"],
];

const SENSITIVE: [string, string, string][] = [
  ["482913", "otp", "Messages.exe"],
  // Fake keys are split so secret scanners don't flag the source.
  ["sk_" + "live_51Hx9QeLkd8sZ2VbQyF3mN0pR7tW4u", "api-key", "chrome.exe"],
  ["ghp" + "_8d1Kq2LmNvX0aZr7YbC4sTe9WfH3jPq6", "token", "Code.exe"],
  ["4111 1111 1111 1111", "card", "chrome.exe"],
];

const IMAGE_PALETTES = [
  ["#1e1b4b", "#6366f1", "#a5b4fc"],
  ["#0f172a", "#0ea5e9", "#bae6fd"],
  ["#052e16", "#22c55e", "#bbf7d0"],
  ["#431407", "#f97316", "#fed7aa"],
  ["#18181b", "#a1a1aa", "#e4e4e7"],
  ["#4a044e", "#d946ef", "#f5d0fe"],
];

function makeImage(w: number, h: number, palette: string[], variant: number): string {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const g = c.getContext("2d");
  if (!g) return "";
  const [dark, mid, light] = palette;
  const s = w / 640;
  if (variant % 3 === 0) {
    // "Screenshot" of an app window
    g.fillStyle = light;
    g.fillRect(0, 0, w, h);
    g.fillStyle = dark;
    g.fillRect(0, 0, w, 36 * s);
    g.fillStyle = mid;
    g.fillRect(0, 36 * s, 150 * s, h);
    g.fillStyle = "rgba(255,255,255,0.85)";
    for (let i = 0; i < 6; i++) g.fillRect(170 * s, (60 + i * 52) * s, (420 - (i % 3) * 70) * s, 34 * s);
    g.fillStyle = "rgba(255,255,255,0.35)";
    for (let i = 0; i < 8; i++) g.fillRect(16 * s, (56 + i * 30) * s, 110 * s, 14 * s);
  } else if (variant % 3 === 1) {
    // Gradient "photo"
    const grad = g.createLinearGradient(0, 0, w, h);
    grad.addColorStop(0, dark);
    grad.addColorStop(0.55, mid);
    grad.addColorStop(1, light);
    g.fillStyle = grad;
    g.fillRect(0, 0, w, h);
    g.fillStyle = "rgba(255,255,255,0.18)";
    g.beginPath();
    g.arc(w * 0.72, h * 0.32, h * 0.22, 0, Math.PI * 2);
    g.fill();
    g.fillStyle = "rgba(0,0,0,0.25)";
    g.beginPath();
    g.moveTo(0, h);
    g.lineTo(w * 0.35, h * 0.55);
    g.lineTo(w * 0.6, h * 0.8);
    g.lineTo(w * 0.85, h * 0.5);
    g.lineTo(w, h * 0.7);
    g.lineTo(w, h);
    g.fill();
  } else {
    // Chart
    g.fillStyle = "#ffffff";
    g.fillRect(0, 0, w, h);
    g.strokeStyle = "#e4e4e7";
    g.lineWidth = Math.max(1, s);
    for (let i = 1; i < 6; i++) {
      g.beginPath();
      g.moveTo(40 * s, (i * h) / 6);
      g.lineTo(w - 20 * s, (i * h) / 6);
      g.stroke();
    }
    const bars = 12;
    for (let i = 0; i < bars; i++) {
      const bh = (0.25 + 0.6 * Math.abs(Math.sin(i * 1.3 + variant))) * h * 0.75;
      g.fillStyle = i % 4 === 3 ? dark : mid;
      const bw = ((w - 80 * s) / bars) * 0.6;
      g.fillRect(50 * s + i * ((w - 80 * s) / bars), h - 20 * s - bh, bw, bh);
    }
  }
  return c.toDataURL("image/png");
}

// ---------- generate ----------
const DAY = 86_400_000;
const NOW = Date.now();
const DEVICE = "desk-7f3a";

function base(i: number, kind: Kind, at: number, app: string | null): Clip {
  return {
    id: `mock-${i.toString().padStart(4, "0")}`,
    kind,
    subkind: null,
    preview: "",
    truncated: false,
    char_count: 0,
    line_count: 0,
    files: [],
    thumb_path: null,
    width: null,
    height: null,
    source_app: app,
    device_id: DEVICE,
    created_at: at,
    last_seen_at: at,
    copy_count: 1,
    pinned: false,
    masked: false,
    expires_at: null,
    text: null,
    html: null,
    image_path: null,
  };
}

function withText(c: Clip, text: string): Clip {
  const preview = text.slice(0, 400);
  return {
    ...c,
    text,
    preview,
    truncated: preview.length < text.length,
    char_count: text.length,
    line_count: text.split("\n").length,
  };
}

let nextId = 0;
function makeClip(at: number): Clip {
  const i = nextId++;
  const r = rnd();
  if (r < 0.22) {
    const [url, app] = pick(LINKS);
    return { ...withText(base(i, "link", at, app), url), subkind: new URL(url).hostname.replace(/^www\./, "") };
  }
  if (r < 0.42) {
    const [lang, code, app] = pick(CODE);
    return { ...withText(base(i, "code", at, app), code), subkind: lang };
  }
  if (r < 0.66) {
    const [t, app] = pick(TEXT);
    return withText(base(i, "text", at, app), t);
  }
  if (r < 0.72) {
    const [t, sub, app] = pick(CONTACTS);
    return { ...withText(base(i, "contact", at, app), t), subkind: sub };
  }
  if (r < 0.79) {
    const [t, sub] = pick(COLORS);
    return { ...withText(base(i, "color", at, pick(["Figma.exe", "Code.exe", "chrome.exe"])), t), subkind: sub };
  }
  if (r < 0.85) {
    const files = pick(FILES);
    return {
      ...base(i, "file", at, "explorer.exe"),
      subkind: "files",
      files,
      preview: files.join("\n"),
      char_count: files.join("\n").length,
      line_count: files.length,
    };
  }
  if (r < 0.965) {
    const v = Math.floor(rnd() * 100);
    const pal = IMAGE_PALETTES[v % IMAGE_PALETTES.length];
    const [w, h] = v % 3 === 0 ? [1920, 1080] : v % 3 === 1 ? [1600, 1200] : [1280, 800];
    const thumb = makeImage(320, Math.round((320 * h) / w), pal, v);
    const full = makeImage(960, Math.round((960 * h) / w), pal, v);
    return {
      ...base(i, "image", at, pick(["SnippingTool.exe", "chrome.exe", "Figma.exe", "ShareX.exe"])),
      subkind: v % 3 === 0 ? "screenshot" : null,
      thumb_path: thumb,
      image_path: full,
      width: w,
      height: h,
    };
  }
  const [secret, sub, app] = pick(SENSITIVE);
  return {
    ...withText(base(i, "sensitive", at, app), secret),
    subkind: sub,
    preview: "••••••",
    truncated: false,
    masked: true,
    copy_count: 1,
    expires_at: at + DAY,
  };
}

function generate(): Clip[] {
  const out: Clip[] = [];
  for (let d = 0; d < 62; d++) {
    // Skip some days entirely; busier recently.
    if (d > 2 && rnd() < 0.18) continue;
    const n = d < 3 ? 9 + Math.floor(rnd() * 6) : Math.floor(rnd() * 5) + (rnd() < 0.2 ? 4 : 0);
    const dayStart = new Date(NOW - d * DAY);
    dayStart.setHours(0, 0, 0, 0);
    for (let k = 0; k < n; k++) {
      const hour = 8 + rnd() * 13;
      let at = dayStart.getTime() + hour * 3_600_000;
      if (at > NOW) at = NOW - Math.floor(rnd() * 3_600_000 * (d === 0 ? 2 : 24));
      out.push(makeClip(at));
    }
  }
  // Some reused clips
  for (const c of out) {
    if (rnd() < 0.15) c.copy_count = 2 + Math.floor(rnd() * 8);
    c.created_at = c.last_seen_at - Math.floor(rnd() * 10) * DAY * (c.copy_count > 1 ? 1 : 0);
  }
  out.sort((a, b) => b.last_seen_at - a.last_seen_at);
  // Pin a few nice ones
  let pinned = 0;
  for (const c of out) {
    if (pinned < 6 && (c.kind === "code" || c.kind === "contact" || c.kind === "link") && rnd() < 0.15) {
      c.pinned = true;
      pinned++;
    }
  }
  // Sensitive clips expire within a day; drop ones that would already be gone.
  for (const c of out) if (c.kind === "sensitive") c.copy_count = 1;
  const live = out.filter((c) => c.kind !== "sensitive" || (c.expires_at ?? 0) > NOW);
  out.length = 0;
  out.push(...live);
  return out.slice(0, 160);
}

const db: Clip[] = generate();

// ---------- event bus ----------
type Cb = (p: unknown) => void;
const subs = new Map<string, Set<Cb>>();
export function subscribe(event: string, cb: Cb): () => void {
  if (!subs.has(event)) subs.set(event, new Set());
  subs.get(event)!.add(cb);
  return () => subs.get(event)?.delete(cb);
}
export function publish(event: string, payload: unknown): void {
  subs.get(event)?.forEach((cb) => cb(payload));
}

// Simulate a new copy now and then so live updates can be seen.
setInterval(() => {
  const c = makeClip(Date.now());
  if (c.kind === "image" || c.kind === "sensitive") return;
  db.unshift(c);
  publish("clips-changed", { outcome: "stored", id: c.id, kind: c.kind });
}, 90_000);

// ---------- commands ----------
function summary(c: Clip) {
  const { text: _t, html: _h, image_path: _i, ...s } = c;
  return s;
}

function list(q: ListQuery) {
  const term = q.search?.trim().toLowerCase();
  let rows = db.filter((c) => {
    if (q.kind && c.kind !== q.kind) return false;
    if (q.pinned_only && !c.pinned) return false;
    if (q.source_app && c.source_app !== q.source_app) return false;
    if (q.from != null && c.last_seen_at < q.from) return false;
    if (q.to != null && c.last_seen_at >= q.to) return false;
    if (q.before != null) {
      if (c.last_seen_at > q.before) return false;
      if (c.last_seen_at === q.before && (q.before_id == null || c.id >= q.before_id)) return false;
    }
    if (term) {
      if (c.masked) return false;
      const hay = `${c.text ?? ""}\n${c.files.join("\n")}\n${c.subkind ?? ""}`.toLowerCase();
      if (!hay.includes(term)) return false;
    }
    return true;
  });
  rows = rows
    .sort((a, b) => b.last_seen_at - a.last_seen_at || (a.id < b.id ? 1 : a.id > b.id ? -1 : 0))
    .slice(0, q.limit ?? 100);
  return rows.map(summary);
}

function dayKey(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function stats(): Stats {
  const byKind = new Map<Kind, number>();
  const apps = new Map<string, number>();
  for (const c of db) {
    byKind.set(c.kind, (byKind.get(c.kind) ?? 0) + 1);
    if (c.source_app) apps.set(c.source_app, (apps.get(c.source_app) ?? 0) + 1);
  }
  return {
    total: db.length,
    by_kind: [...byKind.entries()],
    pinned: db.filter((c) => c.pinned).length,
    top_apps: [...apps.entries()].sort((a, b) => b[1] - a[1]).slice(0, 10),
  };
}

const DEFAULT_EXCLUDED = ["1password.exe", "bitwarden.exe", "keepass.exe", "keepassxc.exe", "lastpass.exe", "dashlane.exe"];
const mockSettings: SettingsView = {
  settings: {
    picker_shortcut: "Ctrl+Shift+Space",
    dashboard_shortcut: "Ctrl+Alt+H",
    theme: "system",
    retention_days: null,
    sensitive_ttl_minutes: 60 * 24,
    excluded_apps: [...DEFAULT_EXCLUDED],
    clean_links: false,
    auto_update_check: null,
  },
  default_excluded_apps: DEFAULT_EXCLUDED,
  autostart: true,
  data_dir: "C:\\Users\\you\\AppData\\Roaming\\ClipTrail",
  version: "0.1.0",
  capture: { paused: false, until: null },
  platform: new URLSearchParams(location.search).get("platform") === "android" ? "android" : "windows",
};
function captureState(): CaptureState {
  const c = mockSettings.capture;
  if (c.paused && c.until != null && Date.now() >= c.until) mockSettings.capture = { paused: false, until: null };
  return mockSettings.capture;
}

function transformText(c: Clip, t: Transform): string {
  if (c.masked || !c.text) throw "Sensitive clips can't be transformed";
  try {
    return applyMock(t, c.text, c.subkind);
  } catch {
    throw "That action doesn't apply to this clip";
  }
}

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function handle(cmd: string, a: Record<string, unknown>): Promise<unknown> {
  await delay(cmd === "list_clips" ? 40 : 10);
  switch (cmd) {
    case "list_clips":
      return list(a.query as ListQuery);
    case "get_clip":
      return db.find((c) => c.id === a.id) ?? null;
    case "stats":
      return stats();
    case "day_counts": {
      const m = new Map<string, number>();
      for (const c of db) {
        if (c.last_seen_at >= (a.from as number) && c.last_seen_at < (a.to as number)) {
          const k = dayKey(c.last_seen_at);
          m.set(k, (m.get(k) ?? 0) + 1);
        }
      }
      return [...m.entries()].sort().map(([day, count]) => ({ day, count })) as DayCount[];
    }
    case "source_apps":
      return stats().top_apps.map(([a]) => a);
    case "set_pinned": {
      const c = db.find((x) => x.id === a.id);
      if (c) c.pinned = a.pinned as boolean;
      return null;
    }
    case "delete_clip": {
      const i = db.findIndex((x) => x.id === a.id);
      if (i >= 0) db.splice(i, 1);
      return null;
    }
    case "clear_count":
      return db.filter((c) => !(a.keepPinned && c.pinned)).length;
    case "clear_history": {
      const before = db.length;
      for (let i = db.length - 1; i >= 0; i--) if (!(a.keepPinned && db[i].pinned)) db.splice(i, 1);
      publish("clips-updated", null);
      return before - db.length;
    }
    case "copy_clip": {
      const c = db.find((x) => x.id === a.id);
      const text = c?.text && a.transform ? transformText(c, a.transform as Transform) : c?.text;
      if (text) await navigator.clipboard?.writeText(text).catch(() => undefined);
      return null;
    }
    case "paste_clip":
      throw "Paste-back isn't available in the browser preview";
    case "transform_clip": {
      const c = db.find((x) => x.id === a.id);
      if (!c) throw "That clip no longer exists";
      return transformText(c, a.transform as Transform);
    }
    case "clip_actions": {
      const c = db.find((x) => x.id === a.id);
      return c && !c.masked && c.text ? availableMock(c.kind, c.subkind, c.text) : [];
    }
    case "open_url":
      window.open(a.url as string, "_blank", "noopener");
      return null;
    case "get_autostart":
      return mockSettings.autostart;
    case "set_autostart":
      mockSettings.autostart = a.enabled as boolean;
      publish("settings-changed", null);
      return null;
    case "capture_state":
      return captureState();
    case "pause_capture":
      mockSettings.capture = {
        paused: true,
        until: a.minutes == null ? null : Date.now() + (a.minutes as number) * 60_000,
      };
      publish("capture-changed", captureState());
      return null;
    case "resume_capture":
      mockSettings.capture = { paused: false, until: null };
      publish("capture-changed", captureState());
      return null;
    case "get_settings":
      return { ...mockSettings, capture: captureState() };
    case "update_settings": {
      const s = a.settings as Settings;
      if (/^(ctrl\+)?alt\+tab$/i.test(s.picker_shortcut) || /^win\+/i.test(s.picker_shortcut)) {
        throw `${s.picker_shortcut} is already in use by another app`;
      }
      mockSettings.settings = { ...s, excluded_apps: [...new Set(s.excluded_apps.map((x) => x.trim().toLowerCase()).filter(Boolean))].sort() };
      publish("settings-changed", null);
      return { ...mockSettings, capture: captureState() };
    }
    case "window_effects":
      return false;
    case "backend_status":
      return null;
    default:
      return null;
  }
}
