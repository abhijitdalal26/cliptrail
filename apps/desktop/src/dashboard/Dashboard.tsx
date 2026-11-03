import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { format, startOfDay } from "date-fns";
import { AlertTriangle, CalendarDays, Flame, Keyboard } from "lucide-react";
import { api, errorMessage, isTauri, listen, onClipsChanged } from "../lib/api";
import { appName } from "../lib/apps";
import { dayKey, plural } from "../lib/format";
import { heatmapStart } from "../lib/heat";
import { isTypingTarget, useDebounced, useEvent, useTick } from "../lib/hooks";
import { KIND_META } from "../lib/kinds";
import { dismiss, toast } from "../lib/toast";
import type { CaptureState, ClipSummary, ListQuery, Stats } from "../lib/types";
import { useClips } from "../lib/useClips";
import type { View } from "../lib/view";
import { Button, Chip, Segmented } from "../components/ui";
import { Toasts } from "../components/Toasts";
import { Heatmap, HeatLegend } from "./Heatmap";
import { Preview } from "./Preview";
import { SettingsDialog } from "./SettingsDialog";
import { ShortcutsSheet } from "./ShortcutsSheet";
import { Sidebar } from "./Sidebar";
import { Timeline, type TimelineHandle } from "./Timeline";
import { UpdateNotices } from "./Updates";
import { TitleBar } from "./TitleBar";

type Range = "any" | "today" | "7d" | "30d";
const DAY = 86_400_000;
const UNDO_MS = 5000;

export function Dashboard() {
  useTick(60_000); // keep relative times fresh

  const [view, setView] = useState<View>({ type: "all" });
  const [app, setApp] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const debounced = useDebounced(search.trim(), 160);
  const [range, setRange] = useState<Range>("any");
  const [jump, setJump] = useState<{ key: string; before: number } | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [modal, setModal] = useState<"settings" | "shortcuts" | null>(null);

  const [stats, setStats] = useState<Stats | null>(null);
  const [apps, setApps] = useState<string[]>([]);
  const [dayCounts, setDayCounts] = useState<Map<string, number> | null>(null);
  const [capture, setCapture] = useState<CaptureState>({ paused: false, until: null });
  const [pickerKey, setPickerKey] = useState("Ctrl+Shift+Space");
  const [backendError, setBackendError] = useState<string | null>(null);
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);

  const searchRef = useRef<HTMLInputElement>(null);
  const timeline = useRef<TimelineHandle>(null);

  // ---------- query ----------
  const query = useMemo<ListQuery>(() => {
    const now = Date.now();
    const from =
      range === "today" ? startOfDay(now).getTime() : range === "7d" ? now - 7 * DAY : range === "30d" ? now - 30 * DAY : null;
    return {
      kind: view.type === "kind" ? view.kind : null,
      pinned_only: view.type === "pinned",
      source_app: app,
      search: debounced || null,
      from,
      before: jump?.before ?? null,
    };
  }, [view, app, debounced, range, jump]);

  const clips = useClips(query);

  // A new filter starts at the newest matching clip.
  useEffect(() => setSelectedId(null), [query]);

  // ---------- pending deletes (undo) ----------
  const [pending, setPending] = useState<Set<string>>(new Set());
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  /** The "Clip deleted · Undo" toast per pending delete, dismissed on undo. */
  const undoToasts = useRef(new Map<string, number>());
  const lastDeleted = useRef<string | null>(null);

  const visible = useMemo(() => clips.items.filter((c) => !pending.has(c.id)), [clips.items, pending]);
  const selected = visible.find((c) => c.id === selectedId) ?? visible[0] ?? null;

  // ---------- meta (sidebar counts, heatmap) ----------
  const refreshMeta = useCallback(() => {
    void api.stats().then(setStats).catch(() => undefined);
    void api.sourceApps().then(setApps).catch(() => undefined);
    const from = heatmapStart().getTime();
    void api
      .dayCounts(from, Date.now() + DAY)
      .then((d) => setDayCounts(new Map(d.map((x) => [x.day, x.count]))))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refreshMeta();
    void api.captureState().then(setCapture).catch(() => undefined);
    void api
      .getSettings()
      .then((v) => {
        setPickerKey(v.settings.picker_shortcut);
        setHotkeyError(v.hotkey_error ?? null);
      })
      .catch(() => undefined);
    void api.backendStatus().then(setBackendError).catch(() => undefined);
    if (isTauri) {
      void api
        .windowEffects()
        .then((on) => document.documentElement.classList.toggle("vibrant", on))
        .catch(() => undefined);
    }
  }, [refreshMeta]);

  const refreshAll = useEvent(() => {
    void clips.refresh();
    refreshMeta();
  });

  useEffect(() => {
    let t: ReturnType<typeof setTimeout> | undefined;
    const un = onClipsChanged(() => {
      clearTimeout(t);
      t = setTimeout(refreshAll, 120);
    });
    const a = listen("capture-changed", (c: CaptureState) => setCapture(c));
    const b = listen("open-settings", () => setModal("settings"));
    const c = listen("settings-changed", () =>
      void api
        .getSettings()
        .then((v) => {
          setPickerKey(v.settings.picker_shortcut);
          setHotkeyError(v.hotkey_error ?? null);
        })
        .catch(() => undefined),
    );
    return () => {
      clearTimeout(t);
      un();
      a();
      b();
      c();
    };
  }, [refreshAll]);

  // Flush pending deletes if the window goes away.
  useEffect(() => {
    const map = timers.current;
    const flush = () => {
      map.forEach((t, id) => {
        clearTimeout(t);
        void api.deleteClip(id);
      });
      map.clear();
    };
    window.addEventListener("beforeunload", flush);
    return () => window.removeEventListener("beforeunload", flush);
  }, []);

  // ---------- actions ----------
  const copy = useEvent((id: string, plain: boolean) => {
    api
      .copyClip(id, plain)
      .then(() => toast(plain ? "Copied as plain text" : "Copied to clipboard"))
      .catch((e) => toast({ message: errorMessage(e), tone: "error" }));
  });

  const togglePin = useEvent((c: ClipSummary) => {
    const pinned = !c.pinned;
    clips.patch(c.id, { pinned });
    api
      .setPinned(c.id, pinned)
      .then(() => {
        toast(pinned ? "Pinned" : "Unpinned");
        refreshMeta();
        if (view.type === "pinned") void clips.refresh();
      })
      .catch((e) => {
        clips.patch(c.id, { pinned: !pinned });
        toast({ message: errorMessage(e), tone: "error" });
      });
  });

  const undoDelete = useEvent((id: string) => {
    const t = timers.current.get(id);
    if (!t) return;
    clearTimeout(t);
    timers.current.delete(id);
    const tid = undoToasts.current.get(id);
    if (tid != null) dismiss(tid);
    undoToasts.current.delete(id);
    setPending((p) => {
      const n = new Set(p);
      n.delete(id);
      return n;
    });
    setSelectedId(id);
  });

  const remove = useEvent((c: ClipSummary) => {
    const idx = visible.findIndex((x) => x.id === c.id);
    const next = visible[idx + 1] ?? visible[idx - 1] ?? null;
    setSelectedId(next?.id ?? null);
    setPending((p) => new Set(p).add(c.id));
    lastDeleted.current = c.id;
    const t = setTimeout(() => {
      timers.current.delete(c.id);
      undoToasts.current.delete(c.id);
      api
        .deleteClip(c.id)
        .then(() => {
          void clips.refresh();
          refreshMeta();
        })
        .catch((e) => toast({ message: errorMessage(e), tone: "error" }))
        .finally(() =>
          setPending((p) => {
            const n = new Set(p);
            n.delete(c.id);
            return n;
          }),
        );
    }, UNDO_MS);
    timers.current.set(c.id, t);
    undoToasts.current.set(
      c.id,
      toast({ message: "Clip deleted", action: { label: "Undo", run: () => undoDelete(c.id) }, duration: UNDO_MS }),
    );
  });

  const select = useCallback((id: string) => setSelectedId(id), []);
  const copyById = useCallback((id: string) => copy(id, false), [copy]);

  const move = (delta: number) => {
    if (!visible.length) return;
    const i = selected ? visible.findIndex((c) => c.id === selected.id) : -1;
    const n = visible[Math.max(0, Math.min(visible.length - 1, i + delta))];
    setSelectedId(n.id);
    timeline.current?.scrollToId(n.id);
    if (i + delta >= visible.length - 3) void clips.loadMore();
  };

  const filtered = view.type !== "all" || !!app || !!debounced || range !== "any" || !!jump;
  const clearFilters = () => {
    setView({ type: "all" });
    setApp(null);
    setRange("any");
    setJump(null);
    setSearch("");
  };

  // ---------- keyboard ----------
  const onKey = useEvent((e: KeyboardEvent) => {
    if (modal) return;
    const typing = isTypingTarget(e.target);
    const inSearch = e.target === searchRef.current;
    const mod = e.ctrlKey || e.metaKey;

    if ((e.key === "/" && !typing) || (mod && e.key.toLowerCase() === "f")) {
      e.preventDefault();
      searchRef.current?.focus();
      searchRef.current?.select();
    } else if (e.key === "?" && !typing) {
      e.preventDefault();
      setModal("shortcuts");
    } else if (mod && e.key === ",") {
      e.preventDefault();
      setModal("settings");
    } else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && (!typing || inSearch)) {
      e.preventDefault();
      move(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter" && (!typing || inSearch) && selected) {
      e.preventDefault();
      copy(selected.id, e.shiftKey);
    } else if (mod && e.key.toLowerCase() === "p") {
      e.preventDefault();
      if (selected) togglePin(selected);
    } else if (e.key === "Delete" && !typing && selected) {
      e.preventDefault();
      remove(selected);
    } else if (mod && e.key.toLowerCase() === "z" && !typing && lastDeleted.current) {
      e.preventDefault();
      undoDelete(lastDeleted.current);
    } else if (e.key === "Escape") {
      if (search) setSearch("");
      else if (filtered) clearFilters();
      else (document.activeElement as HTMLElement | null)?.blur();
    }
  });

  useEffect(() => {
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onKey]);

  // ---------- derived ----------
  const title =
    view.type === "all" ? "All clips" : view.type === "pinned" ? "Pinned" : KIND_META[view.kind].plural;
  const byKind = new Map(stats?.by_kind ?? []);
  const titleCount =
    view.type === "all" ? stats?.total : view.type === "pinned" ? stats?.pinned : byKind.get(view.kind);

  const heat = useMemo(() => {
    if (!dayCounts) return null;
    let total = 0;
    let best: [string, number] | null = null;
    dayCounts.forEach((n, k) => {
      total += n;
      if (!best || n > best[1]) best = [k, n];
    });
    return { total, best: best as [string, number] | null, today: dayCounts.get(dayKey(Date.now())) ?? 0 };
  }, [dayCounts]);

  const pickDay = (ms: number) => {
    setJump({ key: dayKey(ms), before: ms + DAY });
    setRange("any");
    timeline.current?.scrollToTop();
  };

  const top = (
    <div className="px-1 pt-3">
      <div className="flex items-stretch gap-6 rounded-xl border border-line bg-panel-2 px-4 py-3.5">
        <Heatmap counts={dayCounts ?? new Map()} selected={jump?.key ?? null} onPick={pickDay} />
        <div className="flex min-w-0 flex-1 flex-col justify-between gap-3 border-l border-line pl-5">
          <div className="grid grid-cols-[repeat(auto-fit,minmax(104px,1fr))] gap-x-4 gap-y-3">
            <Stat label="Today" value={heat ? plural(heat.today, "clip") : "—"} />
            <Stat label="Last 16 weeks" value={heat ? plural(heat.total, "clip") : "—"} />
            {heat?.best && (
              <Stat
                label="Busiest day"
                icon={<Flame size={12} className="text-[#f97316]" />}
                value={`${format(new Date(heat.best[0] + "T00:00"), "MMM d")} · ${heat.best[1]}`}
              />
            )}
          </div>
          <HeatLegend />
        </div>
      </div>
    </div>
  );

  return (
    <div className="flex h-full flex-col">
      <TitleBar
        ref={searchRef}
        search={search}
        onSearch={setSearch}
        onSearchKey={(e) => {
          if (e.key === "Escape" && search) {
            e.stopPropagation();
            setSearch("");
          }
        }}
      />
      <div className="flex min-h-0 flex-1">
        <Sidebar
          view={view}
          onView={(v) => {
            setView(v);
            timeline.current?.scrollToTop();
          }}
          stats={stats}
          apps={apps}
          app={app}
          onApp={setApp}
          capture={capture}
          onToggleCapture={() => void (capture.paused ? api.resumeCapture() : api.pauseCapture(null))}
          onSettings={() => setModal("settings")}
          onShortcuts={() => setModal("shortcuts")}
        />

        <main className="flex min-w-0 flex-1 overflow-hidden rounded-tl-[14px] border-t border-l border-line bg-panel shadow-card">
          {/* Timeline column */}
          <section className="flex min-w-0 flex-1 flex-col">
            <div className="flex items-center gap-3 px-6 pt-4 pb-2">
              <div className="flex min-w-0 flex-1 items-baseline gap-2">
                <h1 className="truncate text-[17px] font-semibold tracking-[-0.02em] text-fg">{title}</h1>
                {titleCount != null && <span className="text-[12.5px] text-fg-3 tabular-nums">{titleCount.toLocaleString()}</span>}
              </div>
              <Segmented<Range>
                size="sm"
                value={range}
                onChange={(r) => {
                  setRange(r);
                  setJump(null);
                }}
                options={[
                  { value: "any", label: "All time" },
                  { value: "today", label: "Today" },
                  { value: "7d", label: "7 days" },
                  { value: "30d", label: "30 days" },
                ]}
              />
            </div>
            {(app || jump || debounced) && (
              <div className="anim-drop flex flex-wrap items-center gap-1.5 px-6 pb-2">
                {debounced && <Chip active onRemove={() => setSearch("")}>“{debounced}”</Chip>}
                {app && (
                  <Chip active onRemove={() => setApp(null)}>
                    {appName(app)}
                  </Chip>
                )}
                {jump && (
                  <Chip active icon={CalendarDays} onRemove={() => setJump(null)}>
                    {format(new Date(jump.key + "T00:00"), "MMM d")} and earlier
                  </Chip>
                )}
                <button onClick={clearFilters} className="ml-1 text-[12px] font-medium text-fg-3 hover:text-fg">
                  Clear all
                </button>
              </div>
            )}
            {backendError && (
              <div className="mx-6 mb-2 flex items-start gap-2.5 rounded-xl border border-[color-mix(in_oklab,var(--danger)_30%,transparent)] bg-[color-mix(in_oklab,var(--danger)_7%,transparent)] px-3.5 py-2.5 text-[12.5px] text-fg-2">
                <AlertTriangle size={15} className="mt-0.5 shrink-0 text-danger" />
                <span>
                  <span className="font-medium text-fg">The clip store couldn't be opened.</span> {backendError}
                </span>
              </div>
            )}
            {hotkeyError && !backendError && (
              <div className="anim-drop mx-6 mb-2 flex items-center gap-2.5 rounded-xl border border-[color-mix(in_oklab,#f59e0b_35%,transparent)] bg-[color-mix(in_oklab,#f59e0b_8%,transparent)] px-3.5 py-2.5 text-[12.5px] text-fg-2">
                <Keyboard size={15} className="shrink-0 text-[#b45309] dark:text-[#fbbf24]" />
                <span className="min-w-0 flex-1">
                  <span className="font-medium text-fg">A keyboard shortcut isn't working.</span> {hotkeyError}. Pick
                  another combination in Settings.
                </span>
                <Button size="sm" onClick={() => setModal("settings")}>
                  Change shortcut
                </Button>
              </div>
            )}
            <UpdateNotices />
            <div className="min-h-0 flex-1 px-3">
              <Timeline
                ref={timeline}
                items={visible}
                loading={clips.loading}
                hasMore={clips.hasMore}
                filtered={filtered}
                shortcut={pickerKey}
                selectedId={selected?.id ?? null}
                dayCounts={filtered ? null : dayCounts}
                top={!debounced ? top : null}
                onLoadMore={clips.loadMore}
                onSelect={select}
                onCopy={copyById}
                onTogglePin={togglePin}
              />
            </div>
          </section>

          {/* Preview column */}
          <aside className="w-[400px] shrink-0 border-l border-line bg-panel xl:w-[440px]">
            <Preview summary={selected} onCopy={copy} onTogglePin={togglePin} onDelete={remove} toast={(m) => toast(m)} />
          </aside>
        </main>
      </div>

      {modal === "settings" && <SettingsDialog onClose={() => setModal(null)} />}
      {modal === "shortcuts" && <ShortcutsSheet onClose={() => setModal(null)} />}
      <Toasts />
    </div>
  );
}

function Stat({ label, value, icon }: { label: string; value: string; icon?: React.ReactNode }) {
  return (
    <div>
      <div className="text-[11px] font-medium text-fg-3">{label}</div>
      <div className="flex items-center gap-1.5 text-[13px] font-semibold text-fg tabular-nums">
        {icon}
        {value}
      </div>
    </div>
  );
}
