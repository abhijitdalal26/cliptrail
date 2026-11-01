import { memo } from "react";
import { Copy, Pin, PinOff } from "lucide-react";
import { assetUrl } from "../lib/api";
import { appName } from "../lib/apps";
import { cx } from "../lib/cx";
import { parseColor, parseUrl, shortTime, splitPath, untilLabel } from "../lib/format";
import { useHighlighted } from "../lib/highlight";
import { subkindLabel } from "../lib/kinds";
import type { ClipSummary } from "../lib/types";
import { AppAvatar, KindBadge, KindTile } from "../components/ui";
import { DeviceBadge } from "../components/DeviceBadge";

interface Props {
  clip: ClipSummary;
  selected: boolean;
  onSelect: (id: string) => void;
  onCopy: (id: string) => void;
  onTogglePin: (clip: ClipSummary) => void;
}

export const ClipCard = memo(function ClipCard({ clip, selected, onSelect, onCopy, onTogglePin }: Props) {
  return (
    <div
      role="option"
      aria-selected={selected}
      onMouseDown={() => onSelect(clip.id)}
      onDoubleClick={() => onCopy(clip.id)}
      className={cx(
        "group relative flex gap-3 rounded-[11px] px-3 py-2.5 transition-colors duration-150",
        selected
          ? "bg-accent-soft shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--accent)_22%,transparent)]"
          : "hover:bg-hover",
      )}
    >
      <Leading clip={clip} />
      <div className="min-w-0 flex-1">
        <Body clip={clip} />
        <Meta clip={clip} />
      </div>
      <div
        className={cx(
          "hover-tools absolute top-2 right-2 flex items-center gap-0.5 rounded-lg border border-line bg-elevated p-0.5 shadow-card",
          "pointer-events-none opacity-0 transition-opacity duration-150 group-hover:pointer-events-auto group-hover:opacity-100",
        )}
      >
        <MiniAction label="Copy" onClick={() => onCopy(clip.id)}>
          <Copy size={13} strokeWidth={2} />
        </MiniAction>
        <MiniAction label={clip.pinned ? "Unpin" : "Pin"} onClick={() => onTogglePin(clip)}>
          {clip.pinned ? <PinOff size={13} strokeWidth={2} /> : <Pin size={13} strokeWidth={2} />}
        </MiniAction>
      </div>
    </div>
  );
});

function MiniAction({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      aria-label={label}
      title={label}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className="grid h-6 w-6 place-items-center rounded-md text-fg-3 transition-colors hover:bg-hover hover:text-fg"
    >
      {children}
    </button>
  );
}

function Leading({ clip }: { clip: ClipSummary }) {
  if (clip.kind === "color") {
    const c = parseColor(clip.preview);
    return (
      <span className="checker mt-0.5 h-[30px] w-[30px] shrink-0 overflow-hidden rounded-[9px] shadow-[inset_0_0_0_1px_var(--border)]">
        <span className="block h-full w-full" style={{ background: c?.css ?? "transparent" }} />
      </span>
    );
  }
  return <KindTile kind={clip.kind} className="mt-0.5" />;
}

function Body({ clip }: { clip: ClipSummary }) {
  switch (clip.kind) {
    case "link": {
      const u = parseUrl(clip.preview);
      return (
        <div className="min-w-0">
          <div className="truncate text-[13px] font-medium text-fg">{u?.host ?? clip.subkind ?? clip.preview}</div>
          <div className="truncate text-[12.5px] text-fg-3">{u ? u.path || "/" : clip.preview}</div>
        </div>
      );
    }
    case "code":
      return <CodePreview clip={clip} />;
    case "image":
      return (
        <div className="flex items-center gap-3">
          <img
            src={assetUrl(clip.thumb_path)}
            alt=""
            loading="lazy"
            draggable={false}
            className="checker h-[64px] max-w-[160px] rounded-md object-cover shadow-[0_0_0_1px_var(--border)]"
            style={{ aspectRatio: clip.width && clip.height ? `${clip.width} / ${clip.height}` : undefined }}
          />
          <div className="min-w-0 text-[12.5px] text-fg-3">
            <div className="font-medium text-fg">{clip.subkind === "screenshot" ? "Screenshot" : "Image"}</div>
            {clip.width && clip.height ? `${clip.width} × ${clip.height}` : null}
          </div>
        </div>
      );
    case "file": {
      const first = clip.files[0] ?? clip.preview;
      const { name, dir } = splitPath(first);
      return (
        <div className="min-w-0">
          <div className="flex items-baseline gap-1.5">
            <span className="truncate text-[13px] font-medium text-fg">{name}</span>
            {clip.files.length > 1 && (
              <span className="shrink-0 text-[12px] text-fg-3">+{clip.files.length - 1} more</span>
            )}
          </div>
          <div className="truncate text-[12.5px] text-fg-3">{dir}</div>
        </div>
      );
    }
    case "color": {
      const c = parseColor(clip.preview);
      return (
        <div className="min-w-0">
          <div className="code truncate text-[13px] font-medium text-fg">{clip.preview}</div>
          <div className="code truncate text-[12px] text-fg-3">{c && c.hex !== clip.preview.toUpperCase() ? c.hex : c?.rgb}</div>
        </div>
      );
    }
    case "sensitive":
      return (
        <div className="min-w-0">
          <div className="text-[15px] leading-5 tracking-[0.18em] text-fg-2">••••••••••</div>
          <div className="text-[12.5px] text-fg-3">
            {subkindLabel("sensitive", clip.subkind) ?? "Sensitive"}
            {clip.expires_at ? ` · expires in ${untilLabel(clip.expires_at)}` : ""}
          </div>
        </div>
      );
    case "contact":
      return (
        <div className="min-w-0">
          <div className="truncate text-[13px] font-medium text-fg">{clip.preview}</div>
          <div className="text-[12.5px] text-fg-3">{subkindLabel("contact", clip.subkind) ?? "Contact"}</div>
        </div>
      );
    default:
      return (
        <div
          className={cx(
            "line-clamp-3 text-[13px] leading-[1.5] break-words whitespace-pre-line text-fg",
            (clip.truncated || clip.line_count > 3) && "fade-out-b",
          )}
        >
          {clip.preview}
        </div>
      );
  }
}

function CodePreview({ clip }: { clip: ClipSummary }) {
  const lines = clip.preview.split("\n").slice(0, 4).join("\n");
  const html = useHighlighted(lines, clip.subkind);
  const long = clip.line_count > 3 || clip.truncated;
  return (
    <pre
      className={cx(
        "code max-h-[58px] overflow-hidden text-[12px] leading-[19px] whitespace-pre text-fg",
        long && "fade-out-b",
      )}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function Meta({ clip }: { clip: ClipSummary }) {
  const sub = clip.kind === "code" || clip.kind === "image" ? subkindLabel(clip.kind, clip.subkind) : null;
  return (
    <div className="mt-1.5 flex items-center gap-1.5 text-[11.5px] text-fg-3">
      {clip.kind === "code" && <KindBadge kind="code" label={sub ?? "Code"} />}
      {clip.kind === "sensitive" && <KindBadge kind="sensitive" />}
      <AppAvatar app={clip.source_app} size={14} />
      <span className="truncate">{appName(clip.source_app)}</span>
      <DeviceBadge deviceId={clip.device_id} />
      <span className="text-fg-4">·</span>
      <span className="shrink-0 tabular-nums">{shortTime(clip.last_seen_at)}</span>
      {clip.copy_count > 1 && (
        <>
          <span className="text-fg-4">·</span>
          <span className="shrink-0 tabular-nums">×{clip.copy_count}</span>
        </>
      )}
      {clip.pinned && <Pin size={11} strokeWidth={2.4} className="ml-0.5 shrink-0 fill-current text-accent-text" />}
      {clip.kind === "code" && clip.line_count > 1 && (
        <span className="ml-auto shrink-0 tabular-nums">{clip.line_count} lines</span>
      )}
    </div>
  );
}
