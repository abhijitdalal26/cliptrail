import { useRef, useState } from "react";
import {
  ChevronDown,
  Clock,
  Keyboard,
  LayoutDashboard,
  Layers,
  Pause,
  Play,
  Settings,
  Star,
  Trash2,
} from "lucide-react";
import { appName } from "../lib/apps";
import { cx } from "../lib/cx";
import { compact } from "../lib/format";
import { moveKind, useKindOrder } from "../lib/kindOrder";
import { KIND_META } from "../lib/kinds";
import type { CaptureState, Kind, Stats } from "../lib/types";
import { sameView, type View } from "../lib/view";
import { AppAvatar, IconButton } from "../components/ui";
import { SyncStatusPill } from "./SyncStatusPill";

interface Props {
  view: View;
  onView: (v: View) => void;
  stats: Stats | null;
  apps: string[];
  app: string | null;
  onApp: (a: string | null) => void;
  capture: CaptureState;
  onToggleCapture: () => void;
  onSettings: () => void;
  onShortcuts: () => void;
}

export function Sidebar(p: Props) {
  const [allApps, setAllApps] = useState(false);
  const byKind = new Map(p.stats?.by_kind ?? []);
  const appCounts = new Map(p.stats?.top_apps ?? []);
  const apps = allApps ? p.apps : p.apps.slice(0, 6);
  const [kinds, saveKinds] = useKindOrder();
  // Drag to reorder: `drag.to` is where the dragged kind would land.
  const [drag, setDrag] = useState<{ kind: Kind; to: number } | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const dragged = useRef(false);
  const shown = drag ? moveKind(kinds, drag.kind, drag.to) : kinds;

  /** Index (in the list without the dragged row) the pointer is over. */
  const slotAt = (y: number, kind: Kind) => {
    const rows = Array.from(list.current?.children ?? []).filter((el) => (el as HTMLElement).dataset.kind !== kind);
    return rows.filter((el) => {
      const r = el.getBoundingClientRect();
      return r.top + r.height / 2 < y;
    }).length;
  };

  const startDrag = (e: React.PointerEvent, kind: Kind) => {
    if (e.button !== 0) return;
    const startY = e.clientY;
    let to: number | null = null;
    dragged.current = false;
    const move = (ev: PointerEvent) => {
      if (to == null && Math.abs(ev.clientY - startY) < 5) return;
      to = slotAt(ev.clientY, kind);
      dragged.current = true;
      setDrag({ kind, to });
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setDrag(null);
      if (to != null) saveKinds(moveKind(kinds, kind, to));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  /** Alt+↑ / Alt+↓ moves the focused category. */
  const keyMove = (e: React.KeyboardEvent, kind: Kind) => {
    if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    const i = kinds.indexOf(kind);
    saveKinds(moveKind(kinds, kind, e.key === "ArrowUp" ? i - 1 : i + 1));
  };

  return (
    <nav className="flex h-full min-h-0 w-[232px] shrink-0 flex-col">
      <div className="scroll-y no-scrollbar min-h-0 flex-1 px-2.5 pt-1 pb-3">
        <Item
          icon={<LayoutDashboard size={15} strokeWidth={1.9} />}
          label="Dashboard"
          active={sameView(p.view, { type: "dashboard" })}
          onClick={() => p.onView({ type: "dashboard" })}
        />
        <Item
          icon={<Layers size={15} strokeWidth={1.9} />}
          label="All clips"
          count={p.stats?.total}
          active={sameView(p.view, { type: "all" })}
          onClick={() => p.onView({ type: "all" })}
        />
        <Item
          icon={<Star size={15} strokeWidth={1.9} />}
          label="Favorites"
          count={p.stats?.pinned}
          active={sameView(p.view, { type: "favorites" })}
          onClick={() => p.onView({ type: "favorites" })}
        />
        <Item
          icon={<Clock size={15} strokeWidth={1.9} />}
          label="Recent"
          active={sameView(p.view, { type: "recent" })}
          onClick={() => p.onView({ type: "recent" })}
        />
        <Item
          icon={<Trash2 size={15} strokeWidth={1.9} />}
          label="Trash"
          count={p.stats?.trashed}
          active={sameView(p.view, { type: "trash" })}
          onClick={() => p.onView({ type: "trash" })}
        />

        <SectionLabel>Categories</SectionLabel>
        <div ref={list}>
          {shown.map((k) => {
            const Icon = KIND_META[k].icon;
            const n = byKind.get(k) ?? 0;
            return (
              <Item
                key={k}
                kind={k}
                lifted={drag?.kind === k}
                title="Drag to reorder (or Alt+↑ / Alt+↓)"
                onPointerDown={(e) => startDrag(e, k)}
                onKeyDown={(e) => keyMove(e, k)}
                icon={
                  <span className={`k-${k} k-fg`}>
                    <Icon size={15} strokeWidth={1.9} />
                  </span>
                }
                label={KIND_META[k].plural}
                count={n}
                dim={n === 0}
                active={sameView(p.view, { type: "kind", kind: k })}
                onClick={() => {
                  // The click that ends a drag isn't a selection.
                  if (dragged.current) {
                    dragged.current = false;
                    return;
                  }
                  p.onView({ type: "kind", kind: k });
                }}
              />
            );
          })}
        </div>

        {p.apps.length > 0 && (
          <>
            <SectionLabel>Apps</SectionLabel>
            {apps.map((a) => (
              <Item
                key={a}
                icon={<AppAvatar app={a} size={16} />}
                label={appName(a)}
                count={appCounts.get(a)}
                active={p.app === a}
                onClick={() => p.onApp(p.app === a ? null : a)}
              />
            ))}
            {p.apps.length > 6 && (
              <button
                onClick={() => setAllApps((v) => !v)}
                className="flex h-7 w-full items-center gap-2 rounded-lg px-2.5 text-[12px] text-fg-3 hover:text-fg"
              >
                <ChevronDown size={13} className={cx("transition-transform duration-150", allApps && "rotate-180")} />
                {allApps ? "Show less" : `${p.apps.length - 6} more`}
              </button>
            )}
          </>
        )}
      </div>

      <div className="px-2.5 pt-1">
        <SyncStatusPill onOpen={p.onSettings} />
      </div>
      <div className="flex items-center gap-1 px-2.5 pt-1 pb-3">
        <button
          onClick={p.onToggleCapture}
          title={p.capture.paused ? "Resume capture" : "Pause capture"}
          className={cx(
            "group flex h-8 flex-1 items-center gap-2 rounded-lg px-2.5 text-[12px] font-medium transition-colors",
            p.capture.paused
              ? "bg-[color-mix(in_oklab,#f59e0b_14%,transparent)] text-[#b45309] dark:text-[#fbbf24]"
              : "text-fg-2 hover:bg-hover",
          )}
        >
          {p.capture.paused ? (
            <>
              <Play size={13} strokeWidth={2.2} className="shrink-0" />
              <span className="truncate">Paused · Resume</span>
            </>
          ) : (
            <>
              <span className="relative grid h-3.5 w-3.5 shrink-0 place-items-center">
                {/* Static halo: an endless ping kept the GPU busy (~8% of a core) while idle. */}
                <span className="absolute h-3 w-3 rounded-full bg-success opacity-20" />
                <span className="h-[7px] w-[7px] rounded-full bg-success" />
              </span>
              <span className="flex-1 text-left">Capturing</span>
              <Pause
                size={13}
                strokeWidth={2.2}
                className="shrink-0 text-fg-3 opacity-0 transition-opacity group-hover:opacity-100"
              />
            </>
          )}
        </button>
        <IconButton icon={Keyboard} label="Keyboard shortcuts" shortcut="?" tooltipSide="top" onClick={p.onShortcuts} />
        <IconButton icon={Settings} label="Settings" shortcut="Ctrl+," tooltipSide="top" onClick={p.onSettings} />
      </div>
    </nav>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="mt-4 mb-1 px-2.5 text-[11px] font-semibold tracking-[0.04em] text-fg-3 uppercase">{children}</div>
  );
}

function Item({
  icon,
  label,
  count,
  active,
  dim,
  onClick,
  kind,
  lifted,
  title,
  onPointerDown,
  onKeyDown,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  active: boolean;
  dim?: boolean;
  onClick: () => void;
  /** Reorderable category rows. */
  kind?: Kind;
  lifted?: boolean;
  title?: string;
  onPointerDown?: (e: React.PointerEvent) => void;
  onKeyDown?: (e: React.KeyboardEvent) => void;
}) {
  return (
    <button
      onClick={onClick}
      onMouseDown={(e) => e.preventDefault()}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
      data-kind={kind}
      title={title}
      className={cx(
        "flex h-[30px] w-full items-center gap-2.5 rounded-lg px-2.5 text-[13px] transition-colors duration-150",
        active ? "bg-active font-medium text-fg" : "text-fg-2 hover:bg-hover hover:text-fg",
        dim && !active && !lifted && "opacity-55",
        lifted && "relative z-10 bg-panel shadow-soft ring-1 ring-[var(--border-strong)] select-none",
      )}
    >
      <span className="grid w-4 shrink-0 place-items-center">{icon}</span>
      <span className="flex-1 truncate text-left">{label}</span>
      {count != null && count > 0 && (
        <span className={cx("text-[11.5px] tabular-nums", active ? "text-fg-2" : "text-fg-3")}>{compact(count)}</span>
      )}
    </button>
  );
}
