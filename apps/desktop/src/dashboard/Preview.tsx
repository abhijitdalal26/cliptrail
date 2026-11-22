import { useEffect, useRef, useState, type ReactNode } from "react";
import { format } from "date-fns";
import {
  Check,
  Clipboard,
  ClipboardType,
  Copy,
  Download,
  ExternalLink,
  Eye,
  EyeOff,
  FileIcon,
  FolderOpen,
  Loader2,
  Mail,
  MousePointerClick,
  RotateCcw,
  Share2,
  Star,
  Timer,
  Trash2,
} from "lucide-react";
import { api, assetUrl, errorMessage } from "../lib/api";
import { appName } from "../lib/apps";
import { cx } from "../lib/cx";
import {
  fullDate,
  isLightColor,
  parseColor,
  parseUrl,
  plural,
  relative,
  splitPath,
  untilLabel,
} from "../lib/format";
import { useHighlighted } from "../lib/highlight";
import { MOD } from "../lib/hooks";
import { isAndroid } from "../lib/platform";
import { KIND_META, subkindLabel } from "../lib/kinds";
import type { Clip, ClipSummary } from "../lib/types";
import { AppAvatar, Button, IconButton, KindBadge } from "../components/ui";
import {
  platformIcon,
  syncApi,
  useOtherDevice,
  type ImportedFile,
} from "../lib/sync";
import { Actions } from "./Actions";
import { FilePreview } from "../components/FilePreview";
import { mediaKind, remoteImageFile } from "../lib/media";

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

/** The clip panel: title and favorite/delete on top, then the content (click to copy), details and the actions. */
export function Preview({
  summary,
  onCopy,
  onTogglePin,
  onDelete,
  toast,
  trash,
  onRestore,
  onPurge,
}: Props) {
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
  const clip: Clip | null =
    full && full.id === summary.id ? { ...full, ...summary } : null;
  const s = summary;
  const sub = subkindLabel(s.kind, s.subkind);
  // Images copied in a browser remember the page (see clipd-capture cfhtml).
  const page = s.kind === "image" ? parseUrl(s.preview) : null;
  const hasActions =
    !trash && !s.masked && !["sensitive", "image", "file"].includes(s.kind);
  const shownTab: Tab = hasActions ? tab : "details";

  return (
    <div key={s.id} className="anim-fade flex h-full min-h-0 flex-col">
      <div className="scroll-y selectable min-h-0 flex-1 overflow-x-hidden">
        {/* Title, when and where, favorite and delete */}
        <div className="flex items-start gap-2 px-5 pt-5">
          <div className="min-w-0 flex-1">
            <h2 className="line-clamp-2 text-[15.5px] leading-[1.35] font-semibold tracking-[-0.015em] break-words text-fg">
              {clipTitle(s)}
            </h2>
            <MetaLine summary={s} trash={trash} />
          </div>
          {!trash && (
            <div className="-mt-0.5 -mr-1.5 flex shrink-0 items-center">
              <button
                aria-label={
                  s.pinned ? "Remove from favorites" : "Add to favorites"
                }
                aria-pressed={s.pinned}
                title={`${s.pinned ? "Remove from favorites" : "Add to favorites"} (${MOD}+P)`}
                onClick={() => onTogglePin(s)}
                className={cx(
                  "grid h-8 w-8 place-items-center rounded-lg transition-colors hover:bg-hover",
                  s.pinned ? "text-[#f5b301]" : "text-fg-3 hover:text-fg",
                )}
              >
                <Star
                  size={17}
                  strokeWidth={2}
                  className={cx(s.pinned && "fill-current")}
                />
              </button>
              <IconButton
                icon={Trash2}
                label="Move to Trash"
                shortcut="Del"
                tone="danger"
                onClick={() => onDelete(s)}
              />
            </div>
          )}
        </div>

        {/* The content; clicking it copies it (dragging still selects part of it) */}
        <div className="px-5 pt-3.5">
          <CopyArea
            enabled={!trash && !s.masked && s.kind !== "sensitive"}
            onCopy={() => onCopy(s.id, false)}
          >
            <Body summary={s} clip={clip} toast={toast} />
          </CopyArea>
        </div>

        {/* Tabs */}
        <div className="mt-3 flex gap-5 border-b border-line px-5">
          <TabButton
            on={shownTab === "details"}
            onClick={() => setTab("details")}
          >
            Details
          </TabButton>
          {hasActions && (
            <TabButton
              on={shownTab === "actions"}
              onClick={() => setTab("actions")}
            >
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
            {/* No "Unknown app" row: phones can't tell which app a copy came from. */}
            {s.source_app && (
              <Row label="Source">
                <span className="inline-flex items-center gap-1.5">
                  <AppAvatar app={s.source_app} size={14} />
                  {appName(s.source_app)}
                </span>
              </Row>
            )}
            {page && (
              <Row label="From">
                <button
                  className="truncate text-accent-text hover:underline"
                  title={s.preview}
                  onClick={() =>
                    void api
                      .openUrl(s.preview)
                      .catch((e) => toast(errorMessage(e)))
                  }
                >
                  {page.host}
                </button>
              </Row>
            )}
            <DeviceRow deviceId={s.device_id} />
            <Row label={s.kind === "image" ? "Resolution" : "Size"}>
              {sizeLabel(s)}
            </Row>
            <Row label="Copied">
              <span title={fullDate(s.created_at)}>
                {s.copy_count === 1
                  ? `Once · ${shortDate(s.created_at)}`
                  : `${s.copy_count} times · first on ${shortDate(s.created_at)}`}
              </span>
            </Row>
            {clip?.html && <Row label="Format">Rich text</Row>}
            {s.expires_at && (
              <Row label="Expires">in {untilLabel(s.expires_at)}</Row>
            )}
            {trash && s.deleted_at && (
              <Row label="Deleted">{fullDate(s.deleted_at)}</Row>
            )}
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
            <Button
              variant="primary"
              icon={RotateCcw}
              onClick={() => onRestore?.(s)}
              className="h-10 w-full text-[13px]"
            >
              Restore
            </Button>
            <Button
              variant="danger"
              icon={Trash2}
              onClick={() => onPurge?.(s)}
              className="h-9 w-full"
            >
              Delete forever
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="primary"
              icon={Copy}
              onClick={() => onCopy(s.id, false)}
              className="h-10 w-full text-[13px]"
            >
              Copy
            </Button>
            <SecondaryAction
              summary={s}
              clip={clip}
              onCopy={onCopy}
              toast={toast}
            />
            {page && !isAndroid && (
              <Button
                icon={ExternalLink}
                className="h-9 w-full"
                title={s.preview}
                onClick={() =>
                  void api
                    .openUrl(s.preview)
                    .catch((e) => toast(errorMessage(e)))
                }
              >
                Open page
              </Button>
            )}
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
        <Button
          icon={ExternalLink}
          className={cls}
          onClick={() => api.openUrl(url).catch(fail)}
        >
          Open in browser
        </Button>
      );
    }
    case "file":
      if (isAndroid) {
        return remoteImageFile(s.kind, s.files) ? (
          <Button
            icon={Download}
            className={cls}
            onClick={() =>
              syncApi
                .saveFile(s.id, 0)
                .then(() => toast("Saved to Pictures › ClipTrail"))
                .catch(fail)
            }
          >
            Save to phone
          </Button>
        ) : null;
      }
      return s.files.length > 0 ? (
        <Button
          icon={FolderOpen}
          className={cls}
          onClick={() => api.revealPath(s.files[0]).catch(fail)}
        >
          Open in folder
        </Button>
      ) : null;
    case "image":
      if (isAndroid) return null;
      // A screenshot linked to the file the OS saved: show the real file.
      if (s.files.length > 0) {
        return (
          <Button
            icon={FolderOpen}
            title={s.files[0]}
            className={cls}
            onClick={() => api.revealPath(s.files[0]).catch(fail)}
          >
            Open in folder
          </Button>
        );
      }
      return (
        <Button
          icon={Download}
          title="Save a copy of this image as a PNG file"
          className={cls}
          onClick={() =>
            api
              .saveImage(s.id)
              .then((path) => path && toast(`Saved ${splitPath(path).name}`))
              .catch(fail)
          }
        >
          Save as…
        </Button>
      );
    case "contact": {
      const v = clip?.text ?? s.preview;
      return s.subkind === "email" || v.includes("@") ? (
        <Button
          icon={Mail}
          className={cls}
          onClick={() => api.openUrl(`mailto:${v}`).catch(fail)}
        >
          Write email
        </Button>
      ) : null;
    }
    case "sensitive":
      return null;
    default:
      return clip?.html ? (
        <Button
          icon={ClipboardType}
          className={cls}
          onClick={() => onCopy(s.id, true)}
          title="Copy without formatting"
        >
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
      return s.files.length > 1
        ? `${name} and ${s.files.length - 1} more`
        : name;
    }
    case "sensitive":
      return subkindLabel("sensitive", s.subkind) ?? "Sensitive";
    case "code":
      return (
        firstLine(s.preview) || (subkindLabel("code", s.subkind) ?? "Code")
      );
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
  return format(
    d,
    d.getFullYear() === new Date().getFullYear()
      ? "MMM d, h:mm a"
      : "MMM d, yyyy, h:mm a",
  );
}

function TabButton({
  on,
  onClick,
  children,
}: {
  on: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={cx(
        "-mb-px border-b-2 pt-1 pb-2 text-[12.5px] font-medium transition-colors",
        on
          ? "border-accent text-fg"
          : "border-transparent text-fg-3 hover:text-fg-2",
      )}
    >
      {children}
    </button>
  );
}

function sizeLabel(s: ClipSummary): string {
  if (s.kind === "image")
    return s.width && s.height ? `${s.width} × ${s.height}` : "—";
  if (s.kind === "file") return plural(s.files.length, "item");
  if (s.line_count > 1)
    return `${plural(s.char_count, "character")} · ${plural(s.line_count, "line")}`;
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
/** "Copied 2 min ago · Chrome", or the device when the app isn't known. */
function MetaLine({
  summary: s,
  trash,
}: {
  summary: ClipSummary;
  trash?: boolean;
}) {
  const other = useOtherDevice(s.device_id);
  const where = s.source_app
    ? appName(s.source_app)
    : other
      ? `on ${other.name}`
      : null;
  return (
    <div className="mt-1 flex items-center gap-1.5 text-[12px] text-fg-3">
      <span className="shrink-0">
        {trash && s.deleted_at
          ? `Deleted ${relative(s.deleted_at)}`
          : `Copied ${relative(s.last_seen_at)}`}
      </span>
      {where && (
        <>
          <span className="text-fg-4">·</span>
          <span className="truncate">{where}</span>
        </>
      )}
    </div>
  );
}

/** Wraps the clip content: a click copies it, with a brief "Copied" badge. */
function CopyArea({
  enabled,
  onCopy,
  children,
}: {
  enabled: boolean;
  onCopy: () => void;
  children: ReactNode;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef(0);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  if (!enabled) return <>{children}</>;
  return (
    <div
      title="Click to copy"
      onClick={(e) => {
        // Buttons inside the content (colour values, show in folder) do their own thing,
        // and a click that ends a text selection keeps the selection instead.
        if (
          (e.target as HTMLElement).closest(
            "button, a, input, select, textarea",
          )
        )
          return;
        if (window.getSelection()?.toString()) return;
        onCopy();
        setCopied(true);
        window.clearTimeout(timer.current);
        timer.current = window.setTimeout(() => setCopied(false), 1100);
      }}
      className="group/copy relative cursor-pointer rounded-xl transition-shadow hover:shadow-[0_0_0_2px_color-mix(in_oklab,var(--accent)_32%,transparent)]"
    >
      {children}
      <span
        className={cx(
          "pointer-events-none absolute top-2 right-2 inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11.5px] font-semibold shadow-soft transition-opacity duration-150",
          copied
            ? "bg-accent text-accent-fg opacity-100"
            : "bg-panel text-fg-2 opacity-0 group-hover/copy:opacity-100",
        )}
      >
        {copied ? (
          <Check size={13} strokeWidth={2.6} />
        ) : (
          <Copy size={12} strokeWidth={2.2} />
        )}
        {copied ? "Copied" : "Click to copy"}
      </span>
    </div>
  );
}

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
        <div className="text-[13px] font-medium text-fg-2">
          Select a clip to preview it
        </div>
        <div className="mt-1 text-[12px] text-fg-3">
          Use ↑ ↓ to move, Enter to copy
        </div>
      </div>
    </div>
  );
}

function Loading() {
  return (
    <div className="space-y-2.5">
      {[92, 100, 76, 88, 54].map((w, i) => (
        <div
          key={i}
          className="skeleton h-3 rounded"
          style={{ width: `${w}%` }}
        />
      ))}
    </div>
  );
}

function Body({
  summary: s,
  clip,
  toast,
}: {
  summary: ClipSummary;
  clip: Clip | null;
  toast: (m: string) => void;
}) {
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
      return clip ? (
        <CodeBlock code={clip.text ?? s.preview} lang={s.subkind} />
      ) : (
        <Loading />
      );
    case "link":
      return <LinkView url={clip?.text ?? s.preview} />;
    case "color":
      return <ColorView value={clip?.text ?? s.preview} toast={toast} />;
    case "file": {
      if (isAndroid) return <RemoteFiles summary={s} toast={toast} />;
      const media = s.files.find((f) => mediaKind(f));
      return (
        <div className="space-y-3">
          {media && <FilePreview path={media} />}
          <FilesView files={s.files} />
        </div>
      );
    }
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

export function CodeBlock({
  code,
  lang,
}: {
  code: string;
  lang: string | null;
}) {
  const html = useHighlighted(code, lang);
  const lines = code.split("\n").length;
  return (
    <div className="overflow-hidden rounded-xl border border-line bg-panel-2">
      <div className="flex items-center justify-between border-b border-line px-3 py-1.5 text-[11px] text-fg-3">
        <span className="font-medium">
          {subkindLabel("code", lang) ?? "Plain text"}
        </span>
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
  return (
    <div className="overflow-hidden rounded-xl border border-line bg-panel-2">
      <div className="code px-4 py-3 text-[12px] leading-[1.6] break-all text-fg-2">
        {url}
      </div>
    </div>
  );
}

function ColorView({
  value,
  toast,
}: {
  value: string;
  toast: (m: string) => void;
}) {
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
          style={{
            background: c.css,
            color: light ? "rgb(0 0 0 / 0.75)" : "rgb(255 255 255 / 0.92)",
          }}
        >
          <span className="code text-[20px] font-semibold tracking-tight">
            {value}
          </span>
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
            <Clipboard
              size={13}
              className="text-fg-4 opacity-0 transition-opacity group-hover:opacity-100"
            />
          </button>
        ))}
      </div>
    </div>
  );
}

/** Phone: copied files stay on the computer; each one has an Import button, and nothing moves until it's tapped. */
function RemoteFiles({
  summary: s,
  toast,
}: {
  summary: ClipSummary;
  toast: (m: string) => void;
}) {
  const device = useOtherDevice(s.device_id)?.name ?? "your computer";
  const [attempt, setAttempt] = useState(0);
  const image = remoteImageFile(s.kind, s.files);
  return (
    <div className="space-y-3">
      {image && (
        <RemotePreview
          key={`${s.id}:${attempt}`}
          id={s.id}
          device={device}
          onRetry={() => setAttempt((n) => n + 1)}
        />
      )}
      <div className="overflow-hidden rounded-xl border border-line">
        {s.files.map((f, i) => (
          <RemoteFileRow
            key={`${s.id}:${i}`}
            clipId={s.id}
            index={i}
            path={f}
            device={device}
            first={i === 0}
            toast={toast}
          />
        ))}
      </div>
      <div className="text-[12px] leading-[1.45] text-fg-3">
        {`On ${device}. Import brings just that file to this phone (Downloads › ClipTrail); files you don't import never leave your computer.`}
      </div>
    </div>
  );
}

type ImportState =
  | { s: "checking" }
  | { s: "idle" }
  | { s: "importing" }
  | { s: "done"; file: ImportedFile }
  | { s: "failed"; message: string };

function bytesLabel(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** One copied file: Import → Importing… → Open / Share, or the reason it failed with Try again. */
function RemoteFileRow({
  clipId,
  index,
  path,
  device,
  first,
  toast,
}: {
  clipId: string;
  index: number;
  path: string;
  device: string;
  first: boolean;
  toast: (m: string) => void;
}) {
  const [st, setSt] = useState<ImportState>({ s: "checking" });
  const { name, dir } = splitPath(path);
  useEffect(() => {
    let alive = true;
    syncApi.importedFile(clipId, index).then(
      (f) => alive && setSt(f ? { s: "done", file: f } : { s: "idle" }),
      () => alive && setSt({ s: "idle" }),
    );
    return () => {
      alive = false;
    };
  }, [clipId, index]);

  const run = () => {
    setSt({ s: "importing" });
    syncApi.importFile(clipId, index).then(
      (file) => setSt({ s: "done", file }),
      (e) => setSt({ s: "failed", message: errorMessage(e) }),
    );
  };
  const open = (share: boolean) =>
    syncApi.openImported(clipId, index, share).catch((e) => {
      const m = errorMessage(e);
      if (/gone/i.test(m)) setSt({ s: "idle" });
      toast(m);
    });

  return (
    <div className={cx("px-3.5 py-3", !first && "border-t border-line")}>
      <div className="flex items-center gap-3">
        <span className="k-file k-tile grid h-8 w-8 shrink-0 place-items-center rounded-lg">
          <FileIcon size={15} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-medium text-fg">{name}</div>
          <div className="truncate text-[11.5px] text-fg-3">
            {st.s === "done"
              ? `Imported · ${bytesLabel(st.file.size)} · Downloads › ClipTrail`
              : st.s === "importing"
                ? `Getting it from ${device}…`
                : dir}
          </div>
        </div>
        {(st.s === "idle" || st.s === "checking") && (
          <Button
            size="sm"
            icon={Download}
            disabled={st.s === "checking"}
            onClick={run}
          >
            Import
          </Button>
        )}
        {st.s === "importing" && (
          <Loader2
            size={18}
            className="animate-spin text-fg-3"
            aria-label="Importing"
          />
        )}
      </div>
      {st.s === "done" && (
        <div className="mt-2.5 flex gap-2">
          <Button
            size="sm"
            variant="primary"
            icon={ExternalLink}
            className="flex-1"
            onClick={() => open(false)}
          >
            Open
          </Button>
          <Button
            size="sm"
            icon={Share2}
            className="flex-1"
            onClick={() => open(true)}
          >
            Share
          </Button>
        </div>
      )}
      {st.s === "failed" && (
        <div className="mt-2 flex items-center gap-2 text-[12px] leading-[1.4] text-fg-3">
          <span className="min-w-0 flex-1">{st.message}</span>
          <Button size="sm" onClick={run}>
            Try again
          </Button>
        </div>
      )}
    </div>
  );
}

function RemotePreview({
  id,
  device,
  onRetry,
}: {
  id: string;
  device: string;
  onRetry: () => void;
}) {
  const [path, setPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    syncApi.filePreview(id, 0).then(
      (p) => alive && setPath(p),
      (e) => alive && setError(errorMessage(e)),
    );
    return () => {
      alive = false;
    };
  }, [id]);
  return (
    <div className="checker grid min-h-[150px] place-items-center overflow-hidden rounded-xl shadow-[0_0_0_1px_var(--border)]">
      {path ? (
        <img
          src={assetUrl(path)}
          alt=""
          draggable={false}
          className="block h-auto max-h-[42vh] w-full object-contain"
        />
      ) : error ? (
        <div className="px-5 py-6 text-center text-[12.5px] leading-[1.45] text-fg-3">
          {error}
          <div className="mt-2.5">
            <Button size="sm" onClick={onRetry}>
              Try again
            </Button>
          </div>
        </div>
      ) : (
        <div className="text-[12.5px] text-fg-3">
          Getting the preview from {device}…
        </div>
      )}
    </div>
  );
}

function FilesView({
  files,
  reveal = true,
}: {
  files: string[];
  reveal?: boolean;
}) {
  return (
    <div className="overflow-hidden rounded-xl border border-line">
      {files.map((f, i) => {
        const { name, dir } = splitPath(f);
        return (
          <div
            key={f + i}
            className={cx(
              "group flex items-center gap-3 px-3.5 py-2.5",
              i > 0 && "border-t border-line",
            )}
          >
            <span className="k-file k-tile grid h-8 w-8 shrink-0 place-items-center rounded-lg">
              <FileIcon size={15} />
            </span>
            <div className="min-w-0 flex-1">
              <div className="truncate text-[13px] font-medium text-fg">
                {name}
              </div>
              <div className="truncate text-[11.5px] text-fg-3">{dir}</div>
            </div>
            {reveal && (
              <IconButton
                icon={FolderOpen}
                label="Show in folder"
                className="opacity-0 group-hover:opacity-100"
                onClick={() => void api.revealPath(f).catch(() => undefined)}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

function SensitiveView({
  clip,
  summary,
}: {
  clip: Clip | null;
  summary: ClipSummary;
}) {
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
          <div className="text-[20px] tracking-[0.25em] text-fg-3">
            ••••••••••••
          </div>
        )}
        <div className="mt-3 flex items-center gap-1.5 text-[12px] text-fg-3">
          {shown ? <EyeOff size={13} /> : <Eye size={13} />}
          {shown ? "Click to hide" : "Click to reveal"}
        </div>
      </button>
      <div className="flex items-start gap-2.5 rounded-xl bg-hover px-3.5 py-3 text-[12px] leading-[1.5] text-fg-2">
        <Timer size={14} className="mt-0.5 shrink-0 text-fg-3" />
        <span>
          Detected as{" "}
          {subkindLabel("sensitive", summary.subkind)?.toLowerCase() ??
            "sensitive"}
          . Masked, excluded from search
          {summary.expires_at
            ? `, and deleted automatically in ${untilLabel(summary.expires_at)}`
            : ""}
          .
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
        <div className="text-[17px] font-semibold tracking-[-0.01em] break-all text-fg">
          {value}
        </div>
        <div className="mt-1 text-[12px] text-fg-3">
          {email ? "Email address" : "Phone number"}
        </div>
      </div>
    </div>
  );
}
