import { memo } from "react";
import { Copy, Star } from "lucide-react";
import { assetUrl } from "../lib/api";
import { appName } from "../lib/apps";
import { cx } from "../lib/cx";
import { parseColor, parseUrl, shortTime, splitPath, untilLabel } from "../lib/format";
import { useHighlighted } from "../lib/highlight";
import { subkindLabel } from "../lib/kinds";
import type { ClipSummary } from "../lib/types";
import { AppAvatar, KindBadge, KindTile } from "../components/ui";
import { DeviceBadge } from "../components/DeviceBadge";
import { useOtherDevice } from "../lib/sync";

interface Props {
  clip: ClipSummary;
  selected: boolean;
  onSelect: (id: string) => void;
  onCopy: (id: string) => void;
  onTogglePin: (clip: ClipSummary) => void;
  /** Trash: no copy / favorite controls. */
  readOnly?: boolean;
}

export const ClipCard = memo(function ClipCard({ clip, selected, onSelect, onCopy, onTogglePin, readOnly }: Props) {
  return (
    <div
      role="option"
      aria-selected={selected}
      onMouseDown={() => onSelect(clip.id)}
      onDoubleClick={() => !readOnly && onCopy(clip.id)}
      className={cx(
        "group relative flex gap-3 rounded-[11px] px-3 py-2.5 transition-colors duration-150",
        selected
          ? "bg-accent-soft shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--accent)_22%,transparent)]"
          : "hover:bg-hover",
      )}
    >
      <Leading clip={clip} />
      <div className={cx("min-w-0 flex-1", !readOnly && "pr-16")}>
        <Body clip={clip} />
        <Meta clip={clip} />
      </div>
      {!readOnly && (
        <div className="absolute top-2 right-2 flex items-center gap-0.5">
          <RowButton label="Copy" onClick={() => onCopy(clip.id)}>
            <Copy size={15} strokeWidth={2} />
          </RowButton>
          <RowButton
            label={clip.pinned ? "Remove from favorites" : "Add to favorites"}
            pressed={clip.pinned}
            onClick={() => onTogglePin(clip)}
            className={clip.pinned ? "text-[#f5b301] hover:text-[#f5b301]" : undefined}
          >
            <Star size={15} strokeWidth={2} className={cx(clip.pinned && "fill-current")} />
          </RowButton>
        </div>
      )}
    </div>
  );
});

function RowButton({
  label,
  onClick,
  pressed,
  className,
  children,
}: {
  label: string;
  onClick: () => void;
  pressed?: boolean;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      aria-label={label}
      aria-pressed={pressed}
      title={label}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className={cx(
        "grid h-7 w-7 place-items-center rounded-lg text-fg-3 transition-colors hover:bg-hover hover:text-fg",
        className,
      )}
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
  const other = useOtherDevice(clip.device_id);
  const lead = clip.kind === "code" || clip.kind === "sensitive" || !!clip.source_app || !!other;
  return (
    <div className="mt-1.5 flex items-center gap-1.5 text-[11.5px] text-fg-3">
      {clip.kind === "code" && <KindBadge kind="code" label={sub ?? "Code"} />}
      {clip.kind === "sensitive" && <KindBadge kind="sensitive" />}
      {/* No "Unknown app" noise in the list (the clip view still says so). */}
      {clip.source_app && (
        <>
          <AppAvatar app={clip.source_app} size={14} />
          <span className="truncate">{appName(clip.source_app)}</span>
        </>
      )}
      <DeviceBadge deviceId={clip.device_id} />
      {lead && <span className="text-fg-4">·</span>}
      <span className="shrink-0 tabular-nums">{shortTime(clip.last_seen_at)}</span>
      {clip.copy_count > 1 && (
        <>
          <span className="text-fg-4">·</span>
          <span className="shrink-0 tabular-nums">×{clip.copy_count}</span>
        </>
      )}
      {clip.kind === "code" && clip.line_count > 1 && (
        <span className="ml-auto shrink-0 tabular-nums">{clip.line_count} lines</span>
      )}
    </div>
  );
}
