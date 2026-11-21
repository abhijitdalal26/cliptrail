import { memo, useRef, useState } from "react";
import { Check, ChevronRight, Copy, Star } from "lucide-react";
import { assetUrl } from "../lib/api";
import { cx } from "../lib/cx";
import { parseColor, splitPath, untilLabel } from "../lib/format";
import { useHighlighted } from "../lib/highlight";
import { subkindLabel } from "../lib/kinds";
import type { ClipSummary } from "../lib/types";
import { KindTile } from "../components/ui";
import { FileMedia } from "../components/FilePreview";
import { mediaKind } from "../lib/media";
import { isAndroid } from "../lib/platform";

interface Props {
  clip: ClipSummary;
  selected: boolean;
  onSelect: (id: string) => void;
  onCopy: (id: string) => void;
  onTogglePin: (clip: ClipSummary) => void;
  /** Trash: no copy / favorite controls. */
  readOnly?: boolean;
  /** Phone: tapping the row copies it; the chevron or a long press opens it. */
  mobile?: boolean;
}

export const ClipCard = memo(function ClipCard({ clip, selected, onSelect, onCopy, onTogglePin, readOnly, mobile }: Props) {
  if (mobile && !readOnly) return <MobileCard clip={clip} onSelect={onSelect} onCopy={onCopy} />;
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
        {/* Only the content here; app, time, device and counts are in the clip panel. */}
        <Body clip={clip} />
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

const LONG_PRESS_MS = 450;

function MobileCard({
  clip,
  onSelect,
  onCopy,
}: {
  clip: ClipSummary;
  onSelect: (id: string) => void;
  onCopy: (id: string) => void;
}) {
  const [copied, setCopied] = useState(false);
  const press = useRef<{ timer: number; x: number; y: number; long: boolean } | null>(null);
  const flash = useRef(0);

  const cancel = () => {
    if (press.current) window.clearTimeout(press.current.timer);
  };
  // Click (not pointerdown) so a scroll that starts on a row never copies it.
  const tap = () => {
    if (press.current?.long) return;
    onCopy(clip.id);
    setCopied(true);
    window.clearTimeout(flash.current);
    flash.current = window.setTimeout(() => setCopied(false), 900);
  };

  return (
    <div
      role="option"
      aria-selected={false}
      aria-label="Tap to copy"
      onClick={tap}
      onPointerDown={(e) => {
        cancel();
        const p = { timer: 0, x: e.clientX, y: e.clientY, long: false };
        p.timer = window.setTimeout(() => {
          p.long = true;
          onSelect(clip.id);
        }, LONG_PRESS_MS);
        press.current = p;
      }}
      onPointerMove={(e) => {
        const p = press.current;
        if (p && Math.hypot(e.clientX - p.x, e.clientY - p.y) > 8) cancel();
      }}
      onPointerUp={cancel}
      onPointerCancel={cancel}
      onContextMenu={(e) => e.preventDefault()}
      className={cx(
        "relative flex gap-3 rounded-[11px] py-2.5 pr-1 pl-3 transition-colors duration-150 select-none active:bg-active",
        copied && "bg-accent-soft",
      )}
    >
      <Leading clip={clip} />
      <div className="min-w-0 flex-1 self-center">
        <Body clip={clip} />
      </div>
      {copied ? (
        <span className="grid w-10 shrink-0 place-items-center text-accent-text" aria-live="polite">
          <Check size={18} strokeWidth={2.4} aria-label="Copied" />
        </span>
      ) : (
        <button
          aria-label="Open clip"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            onSelect(clip.id);
          }}
          className="grid w-10 shrink-0 place-items-center self-stretch rounded-lg text-fg-3 active:bg-active"
        >
          <ChevronRight size={19} strokeWidth={2} />
        </button>
      )}
    </div>
  );
}

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

/** Copied files: name and folder; images and videos also get a preview from the real file. */
function FileBody({ clip }: { clip: ClipSummary }) {
  const first = clip.files[0] ?? clip.preview;
  const { name, dir } = splitPath(first);
  const [missing, setMissing] = useState(false);
  const text = (
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
  // Phone: the file is on the computer; its preview loads when the clip is opened.
  if (isAndroid || missing || !mediaKind(first)) return text;
  return (
    <div className="flex items-center gap-3">
      <FileMedia
        path={first}
        onMissing={() => setMissing(true)}
        className="checker h-[64px] w-[96px] shrink-0 rounded-md object-cover shadow-[0_0_0_1px_var(--border)]"
      />
      {text}
    </div>
  );
}

function Body({ clip }: { clip: ClipSummary }) {
  switch (clip.kind) {
    case "link":
      // The link exactly as copied, not split into host and path.
      return <div className="line-clamp-2 text-[13px] leading-5 break-all text-fg">{clip.preview}</div>;
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
    case "file":
      return <FileBody clip={clip} />;
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
