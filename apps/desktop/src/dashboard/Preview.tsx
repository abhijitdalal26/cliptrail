import { useEffect, useState, type ReactNode } from "react";
import { format } from "date-fns";
import {
  Clipboard,
  ClipboardType,
  Copy,
  ExternalLink,
  Eye,
  EyeOff,
  FileIcon,
  FolderOpen,
  Mail,
  MousePointerClick,
  RotateCcw,
  Star,
  Timer,
  Trash2,
} from "lucide-react";
import { api, assetUrl, errorMessage } from "../lib/api";
import { appName } from "../lib/apps";
import { cx } from "../lib/cx";
import { fullDate, isLightColor, parseColor, parseUrl, plural, relative, splitPath, untilLabel } from "../lib/format";
import { useHighlighted } from "../lib/highlight";
import { MOD } from "../lib/hooks";
import { isAndroid } from "../lib/platform";
import { KIND_META, subkindLabel } from "../lib/kinds";
import type { Clip, ClipSummary } from "../lib/types";
import { AppAvatar, Button, IconButton, KindBadge } from "../components/ui";
import { platformIcon, useOtherDevice } from "../lib/sync";
import { Actions } from "./Actions";

interface Props {
  summary: ClipSummary | null;
  onCopy: (id: string, plain: boolean) => void;
  onTogglePin: (clip: ClipSummary) => void;
  onDelete: (clip: ClipSummary) => void;
  toast: (msg: string) => void;
  /** Trash view: Restore / Delete forever instead of copy and favorite. */
  trash?: boolean;
  onRestore?: (clip: ClipSummary) => void;
  onPurge?: (clip: ClipSummary) => void;
}

type Tab = "details" | "actions";

/** The clip panel: content on top, then title, details and the actions. */
export function Preview({ summary, onCopy, onTogglePin, onDelete, toast, trash, onRestore, onPurge }: Props) {
  const [full, setFull] = useState<Clip | null>(null);
  const [tab, setTab] = useState<Tab>("details");
  const id = summary?.id;

  useEffect(() => {
    if (!id) return;
    let alive = true;
    api
      .getClip(id)
      .then((c) => alive && setFull(c))
      .catch(() => alive && setFull(null));
    return () => {
      alive = false;
    };
  }, [id]);

  if (!summary) return <NoSelection />;
  const clip: Clip | null = full && full.id === summary.id ? { ...full, ...summary } : null;
  const s = summary;
  const sub = subkindLabel(s.kind, s.subkind);
  const hasActions = !trash && !s.masked && !["sensitive", "image", "file"].includes(s.kind);
  const shownTab: Tab = hasActions ? tab : "details";

  return (
    <div key={s.id} className="anim-fade flex h-full min-h-0 flex-col">
      <div className="scroll-y selectable min-h-0 flex-1 overflow-x-hidden">
        {/* Content first */}
        <div className="px-5 pt-5">
          <Body summary={s} clip={clip} toast={toast} />
        </div>

        {/* Title */}
        <div className="flex items-start gap-2 px-5 pt-4">
          <div className="min-w-0 flex-1">
            <h2 className="line-clamp-2 text-[15.5px] leading-[1.35] font-semibold tracking-[-0.015em] break-words text-fg">
              {clipTitle(s)}
            </h2>
            <div className="mt-1 flex items-center gap-1.5 text-[12px] text-fg-3">
              <span>
                {trash && s.deleted_at ? `Deleted ${relative(s.deleted_at)}` : `Copied ${relative(s.last_seen_at)}`}
              </span>
              {s.source_app && (
                <>
                  <span className="text-fg-4">·</span>
                  <span className="truncate">{appName(s.source_app)}</span>
                </>
              )}
            </div>
          </div>
          {!trash && (
            <div className="-mt-0.5 -mr-1.5 flex shrink-0 items-center">
              <button
                aria-label={s.pinned ? "Remove from favorites" : "Add to favorites"}
                aria-pressed={s.pinned}
                title={`${s.pinned ? "Remove from favorites" : "Add to favorites"} (${MOD}+P)`}
                onClick={() => onTogglePin(s)}
                className={cx(
                  "grid h-8 w-8 place-items-center rounded-lg transition-colors hover:bg-hover",
                  s.pinned ? "text-[#f5b301]" : "text-fg-3 hover:text-fg",
                )}
              >
                <Star size={17} strokeWidth={2} className={cx(s.pinned && "fill-current")} />
              </button>
              <IconButton icon={Trash2} label="Move to Trash" shortcut="Del" tone="danger" onClick={() => onDelete(s)} />
            </div>
          )}
        </div>

        {/* Tabs */}
        <div className="mt-3 flex gap-5 border-b border-line px-5">
          <TabButton on={shownTab === "details"} onClick={() => setTab("details")}>
            Details
          </TabButton>
          {hasActions && (
            <TabButton on={shownTab === "actions"} onClick={() => setTab("actions")}>
              Quick actions
            </TabButton>
          )}
        </div>

        {shownTab === "details" ? (
          <dl className="px-5 py-1 text-[12.5px]">
            <Row label="Category">
              <span className="inline-flex items-center gap-1.5">
                {KIND_META[s.kind].label}
                {sub && <KindBadge kind={s.kind} label={sub} />}
              </span>
            </Row>
            <Row label="Source">
              <span className="inline-flex items-center gap-1.5">
                <AppAvatar app={s.source_app} size={14} />
                {appName(s.source_app)}
              </span>
            </Row>
            <DeviceRow deviceId={s.device_id} />
            <Row label={s.kind === "image" ? "Resolution" : "Size"}>{sizeLabel(s)}</Row>
            <Row label="Copied">
              <span title={fullDate(s.created_at)}>
                {s.copy_count === 1
                  ? `Once · ${shortDate(s.created_at)}`
                  : `${s.copy_count} times · first on ${shortDate(s.created_at)}`}
              </span>
            </Row>
            {clip?.html && <Row label="Format">Rich text</Row>}
            {s.expires_at && <Row label="Expires">in {untilLabel(s.expires_at)}</Row>}
            {trash && s.deleted_at && <Row label="Deleted">{fullDate(s.deleted_at)}</Row>}
          </dl>
        ) : (
          <div className="pt-4">
            <Actions summary={s} original={clip?.text ?? null} bare />
          </div>
        )}
      </div>

      {/* Primary actions */}
      <div className="space-y-2 border-t border-line px-5 py-4">
        {trash ? (
          <>
            <Button variant="primary" icon={RotateCcw} onClick={() => onRestore?.(s)} className="h-10 w-full text-[13px]">
              Restore
            </Button>
            <Button variant="danger" icon={Trash2} onClick={() => onPurge?.(s)} className="h-9 w-full">
              Delete forever
            </Button>
          </>
        ) : (
          <>
            <Button variant="primary" icon={Copy} onClick={() => onCopy(s.id, false)} className="h-10 w-full text-[13px]">
              Copy
            </Button>
            <SecondaryAction summary={s} clip={clip} onCopy={onCopy} toast={toast} />
          </>
        )}
      </div>
    </div>
  );
}

/** The one extra action that fits this kind of clip (open, show, save, plain copy). */
function SecondaryAction({
  summary: s,
  clip,
  onCopy,
  toast,
}: {
  summary: ClipSummary;
  clip: Clip | null;
  onCopy: (id: string, plain: boolean) => void;
  toast: (m: string) => void;
}) {
  const fail = (e: unknown) => toast(errorMessage(e));
  const cls = "h-9 w-full";
  switch (s.kind) {
    case "link": {
      const url = clip?.text ?? s.preview;
      return (
        <Button icon={ExternalLink} className={cls} onClick={() => api.openUrl(url).catch(fail)}>
          Open in browser
        </Button>
      );
    }
    case "file":
      return s.files.length > 0 && !isAndroid ? (
        <Button icon={FolderOpen} className={cls} onClick={() => api.revealPath(s.files[0]).catch(fail)}>
          Open in folder
        </Button>
      ) : null;
    case "image":
      return isAndroid ? null : (
        <Button
          icon={FolderOpen}
          title="Saves a copy to Downloads and shows it in the folder (stored images are encrypted)"
          className={cls}
          onClick={() =>
            api
              .saveImage(s.id)
              .then(() => toast("Saved a copy to Downloads"))
              .catch(fail)
          }
        >
          Open in folder
        </Button>
      );
    case "contact": {
      const v = clip?.text ?? s.preview;
      return s.subkind === "email" || v.includes("@") ? (
        <Button icon={Mail} className={cls} onClick={() => api.openUrl(`mailto:${v}`).catch(fail)}>
          Write email
        </Button>
      ) : null;
    }
    case "sensitive":
      return null;
    default:
      return clip?.html ? (
        <Button icon={ClipboardType} className={cls} onClick={() => onCopy(s.id, true)} title="Copy without formatting">
          Copy as plain text
        </Button>
      ) : null;
  }
}

/** A short, human title for the clip. */
function clipTitle(s: ClipSummary): string {
  switch (s.kind) {
    case "image":
      return s.subkind === "screenshot" ? "Screenshot" : "Image";
    case "link":
      return parseUrl(s.preview)?.host ?? s.preview;
    case "file": {
      const { name } = splitPath(s.files[0] ?? s.preview);
      return s.files.length > 1 ? `${name} and ${s.files.length - 1} more` : name;
    }
    case "sensitive":
      return subkindLabel("sensitive", s.subkind) ?? "Sensitive";
    case "code":
      return firstLine(s.preview) || (subkindLabel("code", s.subkind) ?? "Code");
    default:
      return firstLine(s.preview) || KIND_META[s.kind].label;
  }
}

function firstLine(text: string): string {
  const line = text.split("\n").find((l) => l.trim()) ?? "";
  return line.trim().slice(0, 120);
}

/** "Nov 5, 7:22 PM" (with the year when it isn't this year). */
function shortDate(ms: number): string {
  const d = new Date(ms);
  return format(d, d.getFullYear() === new Date().getFullYear() ? "MMM d, h:mm a" : "MMM d, yyyy, h:mm a");
}

function TabButton({ on, onClick, children }: { on: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      className={cx(
        "-mb-px border-b-2 pt-1 pb-2 text-[12.5px] font-medium transition-colors",
        on ? "border-accent text-fg" : "border-transparent text-fg-3 hover:text-fg-2",
      )}
    >
      {children}
    </button>
  );
}

function sizeLabel(s: ClipSummary): string {
  if (s.kind === "image") return s.width && s.height ? `${s.width} × ${s.height}` : "—";
  if (s.kind === "file") return plural(s.files.length, "item");
  if (s.line_count > 1) return `${plural(s.char_count, "character")} · ${plural(s.line_count, "line")}`;
  return plural(s.char_count, "character");
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-line py-2.5 last:border-b-0">
      <dt className="shrink-0 text-fg-3">{label}</dt>
      <dd className="min-w-0 truncate text-right text-fg">{children}</dd>
    </div>
  );
}

/** Device row in the details list (only shown for clips from another device). */
function DeviceRow({ deviceId }: { deviceId: string }) {
  const d = useOtherDevice(deviceId);
  if (!d) return null;
  const Icon = platformIcon(d.platform);
  return (
    <Row label="Device">
      <span className="inline-flex items-center gap-1.5">
        <Icon size={13} strokeWidth={2} className="shrink-0 text-fg-3" />
        {d.name}
      </span>
    </Row>
  );
}

function NoSelection() {
  return (
    <div className="grid h-full place-items-center px-8 text-center">
      <div>
        <div className="mx-auto mb-3 grid h-11 w-11 place-items-center rounded-xl bg-hover text-fg-3">
          <MousePointerClick size={20} strokeWidth={1.8} />
        </div>
        <div className="text-[13px] font-medium text-fg-2">Select a clip to preview it</div>
        <div className="mt-1 text-[12px] text-fg-3">Use ↑ ↓ to move, Enter to copy</div>
      </div>
    </div>
  );
}

function Loading() {
  return (
    <div className="space-y-2.5">
      {[92, 100, 76, 88, 54].map((w, i) => (
        <div key={i} className="skeleton h-3 rounded" style={{ width: `${w}%` }} />
      ))}
    </div>
  );
}

function Body({ summary: s, clip, toast }: { summary: ClipSummary; clip: Clip | null; toast: (m: string) => void }) {
  switch (s.kind) {
    case "image":
      return (
        <div className="checker overflow-hidden rounded-xl shadow-[0_0_0_1px_var(--border)]">
          <img
            src={assetUrl(clip?.image_path ?? s.thumb_path)}
            alt="Copied image"
            draggable={false}
            className="block h-auto max-h-[42vh] w-full object-contain"
          />
        </div>
      );
    case "code":
      return clip ? <CodeBlock code={clip.text ?? s.preview} lang={s.subkind} /> : <Loading />;
    case "link":
      return <LinkView url={clip?.text ?? s.preview} />;
    case "color":
      return <ColorView value={clip?.text ?? s.preview} toast={toast} />;
    case "file":
      return <FilesView files={s.files} />;
    case "sensitive":
      return <SensitiveView key={s.id} clip={clip} summary={s} />;
    case "contact":
      return <ContactView value={clip?.text ?? s.preview} sub={s.subkind} />;
    default:
      return clip ? (
        <div className="scroll-y max-h-[38vh] rounded-xl border border-line bg-panel-2 px-4 py-3 text-[13.5px] leading-[1.65] break-words whitespace-pre-wrap text-fg">
          {clip.text}
        </div>
      ) : (
        <Loading />
      );
  }
}

export function CodeBlock({ code, lang }: { code: string; lang: string | null }) {
  const html = useHighlighted(code, lang);
  const lines = code.split("\n").length;
  return (
    <div className="overflow-hidden rounded-xl border border-line bg-panel-2">
      <div className="flex items-center justify-between border-b border-line px-3 py-1.5 text-[11px] text-fg-3">
        <span className="font-medium">{subkindLabel("code", lang) ?? "Plain text"}</span>
        <span className="tabular-nums">{plural(lines, "line")}</span>
      </div>
      <div className="scroll-y flex max-h-[38vh] overflow-x-auto">
        <pre
          aria-hidden
          className="code sticky left-0 shrink-0 border-r border-line bg-panel-2 px-2.5 py-3 text-right text-[12px] leading-[20px] text-fg-4 select-none"
        >
          {Array.from({ length: lines }, (_, i) => i + 1).join("\n")}
        </pre>
        <pre
          className="code min-w-0 flex-1 px-3.5 py-3 text-[12px] leading-[20px] whitespace-pre text-fg"
          dangerouslySetInnerHTML={{ __html: html }}
        />
      </div>
    </div>
  );
}

function LinkView({ url }: { url: string }) {
  const u = parseUrl(url);
  return (
    <div>
      <div className="overflow-hidden rounded-xl border border-line bg-panel-2">
        <div className="k-link flex items-center gap-3 border-b border-line px-4 py-3.5">
          <span className="k-tile grid h-9 w-9 place-items-center rounded-[10px] text-[15px] font-semibold uppercase">
            {(u?.host ?? "?").charAt(0)}
          </span>
          <div className="min-w-0">
            <div className="truncate text-[14px] font-semibold text-fg">{u?.host ?? "Link"}</div>
            <div className="truncate text-[12px] text-fg-3">{u?.path || "/"}</div>
          </div>
        </div>
        <div className="code px-4 py-3 text-[12px] leading-[1.6] break-all text-fg-2">{url}</div>
      </div>
    </div>
  );
}

function ColorView({ value, toast }: { value: string; toast: (m: string) => void }) {
  const c = parseColor(value);
  if (!c) return <div className="code text-[13px]">{value}</div>;
  const light = isLightColor(c.hex);
  const rows: [string, string][] = [
    ["HEX", c.hex],
    ["RGB", c.rgb],
    ["HSL", c.hsl],
  ];
  return (
    <div className="space-y-3">
      <div className="checker overflow-hidden rounded-xl shadow-[inset_0_0_0_1px_var(--border)]">
        <div
          className="flex h-40 items-end p-4"
          style={{ background: c.css, color: light ? "rgb(0 0 0 / 0.75)" : "rgb(255 255 255 / 0.92)" }}
        >
          <span className="code text-[20px] font-semibold tracking-tight">{value}</span>
        </div>
      </div>
      <div className="divide-y divide-[var(--border)] overflow-hidden rounded-xl border border-line">
        {rows.map(([k, v]) => (
          <button
            key={k}
            onClick={() =>
              navigator.clipboard
                .writeText(v)
                .then(() => toast(`Copied ${k}`))
                .catch(() => undefined)
            }
            className="group flex w-full items-center gap-3 px-3.5 py-2.5 text-left transition-colors hover:bg-hover"
          >
            <span className="w-9 text-[11px] font-semibold text-fg-3">{k}</span>
            <span className="code flex-1 text-[12.5px] text-fg">{v}</span>
            <Clipboard size={13} className="text-fg-4 opacity-0 transition-opacity group-hover:opacity-100" />
          </button>
        ))}
      </div>
    </div>
  );
}

function FilesView({ files }: { files: string[] }) {
  return (
    <div className="overflow-hidden rounded-xl border border-line">
      {files.map((f, i) => {
        const { name, dir } = splitPath(f);
        return (
          <div
            key={f + i}
            className={cx("group flex items-center gap-3 px-3.5 py-2.5", i > 0 && "border-t border-line")}
          >
            <span className="k-file k-tile grid h-8 w-8 shrink-0 place-items-center rounded-lg">
              <FileIcon size={15} />
            </span>
            <div className="min-w-0 flex-1">
              <div className="truncate text-[13px] font-medium text-fg">{name}</div>
              <div className="truncate text-[11.5px] text-fg-3">{dir}</div>
            </div>
            <IconButton
              icon={FolderOpen}
              label="Show in folder"
              className="opacity-0 group-hover:opacity-100"
              onClick={() => void api.revealPath(f).catch(() => undefined)}
            />
          </div>
        );
      })}
    </div>
  );
}

function SensitiveView({ clip, summary }: { clip: Clip | null; summary: ClipSummary }) {
  const [shown, setShown] = useState(false);
  return (
    <div className="space-y-3">
      <button
        onClick={() => setShown((v) => !v)}
        className="k-sensitive group relative w-full overflow-hidden rounded-xl border border-line bg-panel-2 px-4 py-5 text-left transition-colors hover:border-line-strong"
      >
        {shown && clip?.text ? (
          <div className="code text-[14px] break-all text-fg">{clip.text}</div>
        ) : (
          <div className="text-[20px] tracking-[0.25em] text-fg-3">••••••••••••</div>
        )}
        <div className="mt-3 flex items-center gap-1.5 text-[12px] text-fg-3">
          {shown ? <EyeOff size={13} /> : <Eye size={13} />}
          {shown ? "Click to hide" : "Click to reveal"}
        </div>
      </button>
      <div className="flex items-start gap-2.5 rounded-xl bg-hover px-3.5 py-3 text-[12px] leading-[1.5] text-fg-2">
        <Timer size={14} className="mt-0.5 shrink-0 text-fg-3" />
        <span>
          Detected as {subkindLabel("sensitive", summary.subkind)?.toLowerCase() ?? "sensitive"}. Masked, excluded from
          search
          {summary.expires_at ? `, and deleted automatically in ${untilLabel(summary.expires_at)}` : ""}.
        </span>
      </div>
    </div>
  );
}

function ContactView({ value, sub }: { value: string; sub: string | null }) {
  const email = sub === "email" || value.includes("@");
  return (
    <div>
      <div className="k-contact rounded-xl border border-line bg-panel-2 px-4 py-4">
        <div className="text-[17px] font-semibold tracking-[-0.01em] break-all text-fg">{value}</div>
        <div className="mt-1 text-[12px] text-fg-3">{email ? "Email address" : "Phone number"}</div>
      </div>
    </div>
  );
}
