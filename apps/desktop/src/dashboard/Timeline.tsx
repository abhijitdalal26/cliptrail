import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ClipboardCopy, SearchX } from "lucide-react";
import { dayKey, dayLabel, plural } from "../lib/format";
import { MOD } from "../lib/hooks";
import type { ClipSummary } from "../lib/types";
import { Kbd } from "../components/ui";
import { ClipCard } from "./ClipCard";

type Row =
  | { type: "top" }
  | { type: "header"; key: string; label: string; count: number | null; first: boolean }
  | { type: "clip"; clip: ClipSummary }
  | { type: "more" };

export interface TimelineHandle {
  scrollToId: (id: string) => void;
  scrollToTop: () => void;
}

interface Props {
  items: ClipSummary[];
  loading: boolean;
  hasMore: boolean;
  filtered: boolean;
  shortcut: string;
  selectedId: string | null;
  dayCounts: Map<string, number> | null;
  top: ReactNode;
  onLoadMore: () => void;
  onSelect: (id: string) => void;
  onCopy: (id: string) => void;
  onTogglePin: (c: ClipSummary) => void;
}

export const Timeline = forwardRef<TimelineHandle, Props>(function Timeline(p, ref) {
  const scrollRef = useRef<HTMLDivElement>(null);

  const rows = useMemo<Row[]>(() => {
    const out: Row[] = [{ type: "top" }];
    let last = "";
    for (const c of p.items) {
      const k = dayKey(c.last_seen_at);
      if (k !== last) {
        out.push({
          type: "header",
          key: k,
          label: dayLabel(c.last_seen_at),
          count: p.dayCounts?.get(k) ?? null,
          first: last === "",
        });
        last = k;
      }
      out.push({ type: "clip", clip: c });
    }
    if (p.hasMore && p.items.length > 0) out.push({ type: "more" });
    return out;
  }, [p.items, p.hasMore, p.dayCounts]);

  const v = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => {
      const r = rows[i];
      if (r.type === "top") return 190;
      if (r.type === "header") return 40;
      if (r.type === "more") return 56;
      return r.clip.kind === "image" ? 96 : r.clip.kind === "code" || r.clip.kind === "text" ? 94 : 70;
    },
    overscan: 8,
    getItemKey: (i) => {
      const r = rows[i];
      return r.type === "clip" ? r.clip.id : r.type === "header" ? `h:${r.key}` : r.type;
    },
  });

  useImperativeHandle(
    ref,
    () => ({
      scrollToId: (id) => {
        const i = rows.findIndex((r) => r.type === "clip" && r.clip.id === id);
        if (i >= 0) v.scrollToIndex(i, { align: "auto" });
      },
      scrollToTop: () => v.scrollToOffset(0),
    }),
    [rows, v],
  );

  // Infinite scroll: load the next page when the "more" row is rendered.
  const vItems = v.getVirtualItems();
  const lastIndex = vItems.length ? vItems[vItems.length - 1].index : -1;
  const { hasMore, loading, onLoadMore } = p;
  useEffect(() => {
    if (hasMore && !loading && lastIndex >= rows.length - 4) onLoadMore();
  }, [lastIndex, rows.length, hasMore, loading, onLoadMore]);

  const empty = !p.loading && p.items.length === 0;

  return (
    <div ref={scrollRef} className="scroll-y h-full min-h-0 [overflow-anchor:none]" role="listbox" aria-label="Clips">
      <div className="relative w-full" style={{ height: v.getTotalSize() }}>
        {vItems.map((vi) => {
          const r = rows[vi.index];
          return (
            <div
              key={vi.key}
              data-index={vi.index}
              ref={v.measureElement}
              className="absolute top-0 left-0 w-full px-3"
              style={{ transform: `translateY(${vi.start}px)` }}
            >
              {r.type === "top" && (
                <>
                  {p.top}
                  {empty && <Empty filtered={p.filtered} shortcut={p.shortcut} />}
                  {p.loading && p.items.length === 0 && <Skeleton />}
                </>
              )}
              {r.type === "header" && (
                <div className="flex items-baseline gap-2 px-3 pt-4 pb-1.5">
                  <span className="text-[12.5px] font-semibold tracking-[-0.005em] text-fg">{r.label}</span>
                  {r.count != null && <span className="text-[11.5px] text-fg-3">{plural(r.count, "clip")}</span>}
                </div>
              )}
              {r.type === "clip" && (
                <div className="pb-0.5">
                  <ClipCard
                    clip={r.clip}
                    selected={r.clip.id === p.selectedId}
                    onSelect={p.onSelect}
                    onCopy={p.onCopy}
                    onTogglePin={p.onTogglePin}
                  />
                </div>
              )}
              {r.type === "more" && (
                <div className="flex h-14 items-center justify-center gap-2 text-[12px] text-fg-3">
                  <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-line-strong border-t-accent" />
                  Loading older clips…
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
});

function Skeleton() {
  return (
    <div className="space-y-1 px-3 pt-4">
      {Array.from({ length: 6 }, (_, i) => (
        <div key={i} className="flex gap-3 py-2.5">
          <div className="skeleton h-[30px] w-[30px] rounded-[9px]" />
          <div className="flex-1 space-y-2 pt-1">
            <div className="skeleton h-3 w-3/5 rounded" />
            <div className="skeleton h-2.5 w-2/5 rounded" />
          </div>
        </div>
      ))}
    </div>
  );
}

function Empty({ filtered, shortcut }: { filtered: boolean; shortcut: string }) {
  if (filtered) {
    return (
      <div className="anim-fade flex flex-col items-center px-8 pt-16 pb-10 text-center">
        <div className="mb-3 grid h-12 w-12 place-items-center rounded-2xl bg-hover text-fg-3">
          <SearchX size={22} strokeWidth={1.8} />
        </div>
        <div className="text-[14px] font-semibold text-fg">No clips match</div>
        <div className="mt-1 max-w-[300px] text-[12.5px] text-fg-3">
          Try a shorter search, or clear the filters to see everything.
        </div>
      </div>
    );
  }
  return (
    <div className="anim-fade flex flex-col items-center px-8 pt-14 pb-10 text-center">
      <div className="relative mb-5 h-[76px] w-[96px]">
        <div className="absolute inset-x-5 top-0 h-10 rounded-xl border border-line bg-panel-2" />
        <div className="absolute inset-x-2.5 top-3 h-11 rounded-xl border border-line bg-panel shadow-soft" />
        <div className="absolute inset-x-0 top-7 grid h-12 place-items-center rounded-xl border border-line bg-elevated shadow-card">
          <ClipboardCopy size={20} strokeWidth={1.8} className="text-accent-text" />
        </div>
      </div>
      <div className="text-[15px] font-semibold tracking-[-0.01em] text-fg">Copy anything — it shows up here</div>
      <div className="mt-1.5 max-w-[340px] text-[12.5px] leading-[1.55] text-fg-3">
        Links, code, text, colors, files and screenshots are saved as you copy, grouped by day and searchable.
      </div>
      <div className="mt-4 flex items-center gap-1.5 text-[12px] text-fg-3">
        Paste from history anywhere with
        {shortcut.split("+").map((k) => (
          <Kbd key={k}>{k === "Ctrl" ? MOD : k}</Kbd>
        ))}
      </div>
    </div>
  );
}
