import { useEffect, useState, type ReactNode } from "react";
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
  Pin,
  PinOff,
  Timer,
  Trash2,
} from "lucide-react";
import { api, assetUrl, errorMessage } from "../lib/api";
import { appName } from "../lib/apps";
import { cx } from "../lib/cx";
import { fullDate, isLightColor, parseColor, parseUrl, plural, relative, splitPath, untilLabel } from "../lib/format";
import { useHighlighted } from "../lib/highlight";
import { MOD } from "../lib/hooks";
import { KIND_META, subkindLabel } from "../lib/kinds";
import type { Clip, ClipSummary } from "../lib/types";
import { AppAvatar, Button, IconButton, KindBadge, KindTile } from "../components/ui";
import { DeviceMetaRow } from "../components/DeviceBadge";
import { Actions } from "./Actions";

interface Props {
  summary: ClipSummary | null;
  onCopy: (id: string, plain: boolean) => void;
  onTogglePin: (clip: ClipSummary) => void;
  onDelete: (clip: ClipSummary) => void;
  toast: (msg: string) => void;
}

export function Preview({ summary, onCopy, onTogglePin, onDelete, toast }: Props) {
  const [full, setFull] = useState<Clip | null>(null);
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

  return (
    <div key={s.id} className="anim-fade flex h-full min-h-0 flex-col">
      {/* Header */}
      <div className="flex items-center gap-3 px-5 pt-4 pb-3">
        <KindTile kind={s.kind} size={34} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-[14px] font-semibold tracking-[-0.01em] text-fg">{KIND_META[s.kind].label}</span>
            {sub && <KindBadge kind={s.kind} label={sub} />}
          </div>
          <div className="text-[12px] text-fg-3">{relative(s.last_seen_at)}</div>
        </div>
        <div className="flex items-center gap-0.5">
          <IconButton
            icon={s.pinned ? PinOff : Pin}
            label={s.pinned ? "Unpin" : "Pin"}
            shortcut={`${MOD}+P`}
            active={s.pinned}
            onClick={() => onTogglePin(s)}
          />
          <IconButton icon={Trash2} label="Delete" shortcut="Del" tone="danger" onClick={() => onDelete(s)} />
        </div>
      </div>

      <div className="flex gap-2 px-5 pb-4">
        <Button variant="primary" icon={Copy} onClick={() => onCopy(s.id, false)} className="flex-1">
          Copy
        </Button>
        {s.kind !== "image" && (
          <Button icon={ClipboardType} onClick={() => onCopy(s.id, true)} className="flex-1" title="Copy without formatting">
            Copy as plain text
          </Button>
        )}
      </div>

      <Actions summary={s} original={clip?.text ?? null} />

      <div className="mx-5 h-px bg-line" />

      {/* Body */}
      <div className="scroll-y selectable min-h-0 flex-1 px-5 py-4">
        <Body summary={s} clip={clip} toast={toast} />
      </div>

      {/* Metadata */}
      <div className="border-t border-line px-5 py-3.5">
        <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 text-[12px]">
          <Meta label="Source">
            <span className="inline-flex items-center gap-1.5">
              <AppAvatar app={s.source_app} size={14} />
              {appName(s.source_app)}
            </span>
          </Meta>
          <DeviceMetaRow deviceId={s.device_id} />
          <Meta label="Copied">{s.copy_count === 1 ? "Once" : `${s.copy_count} times`}</Meta>
          <Meta label="First copied">{fullDate(s.created_at)}</Meta>
          {s.copy_count > 1 && <Meta label="Last copied">{fullDate(s.last_seen_at)}</Meta>}
          <Meta label="Size">{sizeLabel(s)}</Meta>
          {s.expires_at && <Meta label="Expires">in {untilLabel(s.expires_at)}</Meta>}
        </dl>
      </div>
    </div>
  );
}

function sizeLabel(s: ClipSummary): string {
  if (s.kind === "image") return s.width && s.height ? `${s.width} × ${s.height} px` : "—";
  if (s.kind === "file") return plural(s.files.length, "item");
  if (s.line_count > 1) return `${plural(s.char_count, "character")} · ${plural(s.line_count, "line")}`;
  return plural(s.char_count, "character");
}

function Meta({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-fg-3">{label}</dt>
      <dd className="min-w-0 truncate text-fg-2">{children}</dd>
    </>
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
            className="block h-auto max-h-[60vh] w-full object-contain"
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
        <div className="text-[13.5px] leading-[1.65] break-words whitespace-pre-wrap text-fg">{clip.text}</div>
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
      <div className="scroll-y flex overflow-x-auto">
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
  const [err, setErr] = useState<string | null>(null);
  return (
    <div className="space-y-3">
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
      <Button
        icon={ExternalLink}
        onClick={() => api.openUrl(url).catch((e) => setErr(errorMessage(e)))}
        className="w-full"
      >
        Open in browser
      </Button>
      {err && <div className="text-[12px] text-danger">{err}</div>}
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
    <div className="space-y-3">
      <div className="k-contact rounded-xl border border-line bg-panel-2 px-4 py-4">
        <div className="text-[17px] font-semibold tracking-[-0.01em] break-all text-fg">{value}</div>
        <div className="mt-1 text-[12px] text-fg-3">{email ? "Email address" : "Phone number"}</div>
      </div>
      {email && (
        <Button icon={Mail} className="w-full" onClick={() => void api.openUrl(`mailto:${value}`).catch(() => undefined)}>
          Write email
        </Button>
      )}
    </div>
  );
}
