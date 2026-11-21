import { useRef, useState } from "react";
import { FileX } from "lucide-react";
import { fileUrl, isTauri } from "../lib/api";
import { splitPath } from "../lib/format";
import { mediaKind } from "../lib/media";

/**
 * A copied image or video shown from where it lives. Videos play muted while
 * hovered. `onMissing` fires when the file can't be loaded (moved, deleted,
 * too big), so the caller can fall back to its icon.
 */
export function FileMedia({
  path,
  className,
  onMissing,
}: {
  path: string;
  className?: string;
  onMissing?: () => void;
}) {
  const video = useRef<HTMLVideoElement>(null);
  const kind = mediaKind(path);
  if (!kind || !isTauri) return null;
  const src = fileUrl(path);
  if (kind === "image") {
    return <img src={src} alt="" loading="lazy" draggable={false} className={className} onError={onMissing} />;
  }
  return (
    <video
      ref={video}
      src={src}
      muted
      loop
      playsInline
      preload="metadata"
      className={className}
      onError={onMissing}
      onMouseEnter={() => void video.current?.play().catch(() => undefined)}
      onMouseLeave={() => {
        const v = video.current;
        if (v) {
          v.pause();
          v.currentTime = 0;
        }
      }}
    />
  );
}

/** Large preview for the clip panel, with a "File not found" state. */
export function FilePreview({ path }: { path: string }) {
  const [missing, setMissing] = useState(false);
  if (!mediaKind(path) || !isTauri) return null;
  if (missing) {
    return (
      <div className="flex items-center gap-2.5 rounded-xl border border-line bg-panel-2 px-4 py-3 text-[12.5px] text-fg-3">
        <FileX size={16} className="shrink-0" />
        <span className="min-w-0 truncate">
          <span className="font-medium text-fg-2">File not found</span> · {splitPath(path).name}
        </span>
      </div>
    );
  }
  return (
    <div className="checker overflow-hidden rounded-xl shadow-[0_0_0_1px_var(--border)]">
      <FileMedia
        key={path}
        path={path}
        onMissing={() => setMissing(true)}
        className="block h-auto max-h-[42vh] w-full object-contain"
      />
    </div>
  );
}
