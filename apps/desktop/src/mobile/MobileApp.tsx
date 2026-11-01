import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, ArrowLeft, ChevronRight, ClipboardPlus, Layers, Pin, Search, Settings, X, Zap } from "lucide-react";
import { api, errorMessage, isTauri, onClipsChanged } from "../lib/api";
import { androidApi, type AndroidStatus } from "../lib/android";
import { cx } from "../lib/cx";
import { useDebounced, useEvent, useTick } from "../lib/hooks";
import { KIND_META } from "../lib/kinds";
import { isAndroid } from "../lib/platform";
import { syncApi, useSyncView } from "../lib/sync";
import { useTheme } from "../lib/theme";
import { toast } from "../lib/toast";
import { ALL_KINDS, type ClipSummary, type Kind, type ListQuery, type Stats } from "../lib/types";
import { useClips } from "../lib/useClips";
import { Toasts } from "../components/Toasts";
import { Preview } from "../dashboard/Preview";
import { Timeline, type TimelineHandle } from "../dashboard/Timeline";
import { MobileSettings } from "./MobileSettings";

type Filter = "all" | "pinned" | Kind;
const UNDO_MS = 5000;

/** Phone layout: search on top, kind chips, day-grouped list, full-screen clip sheet, settings sheet. */
export function MobileApp() {
  useTick(60_000);
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const debounced = useDebounced(search.trim(), 160);
  const [openId, setOpenId] = useState<string | null>(null);
  const [settings, setSettings] = useState(false);
  const [pairLink, setPairLink] = useState<string | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [status, setStatus] = useState<AndroidStatus | null>(null);
  const [backendError, setBackendError] = useState<string | null>(null);
  const timeline = useRef<TimelineHandle>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const { dark } = useTheme();

  useEffect(() => {
    document.documentElement.classList.add("mobile");
  }, []);

  // Status / navigation bars follow the theme.
  useEffect(() => {
    if (isTauri && isAndroid) void api.setWindowTheme(dark).catch(() => undefined);
  }, [dark]);

  const query = useMemo<ListQuery>(
    () => ({
      kind: filter !== "all" && filter !== "pinned" ? filter : null,
      pinned_only: filter === "pinned",
      search: debounced || null,
    }),
    [filter, debounced],
  );
  const clips = useClips(query);

  // ---------- pending deletes (undo) ----------
  const [pending, setPending] = useState<Set<string>>(new Set());
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const visible = useMemo(() => clips.items.filter((c) => !pending.has(c.id)), [clips.items, pending]);
  const open = clips.items.find((c) => c.id === openId) ?? null;

  const refreshMeta = useCallback(() => {
    void api.stats().then(setStats).catch(() => undefined);
  }, []);

  const refreshStatus = useCallback(() => {
    if (!isAndroid) return;
    void androidApi.status().then(setStatus).catch(() => undefined);
  }, []);

  const checkPairLink = useCallback(() => {
    if (!isAndroid) return;
    void androidApi
      .takePairLink()
      .then((r) => {
        if (r.link) {
          setOpenId(null);
          setPairLink(r.link);
          setSettings(true);
        }
      })
      .catch(() => undefined);
  }, []);

  const refreshAll = useEvent(() => {
    void clips.refresh();
    refreshMeta();
  });

  useEffect(() => {
    refreshMeta();
    refreshStatus();
    checkPairLink();
    void api.backendStatus().then(setBackendError).catch(() => undefined);
    let t: ReturnType<typeof setTimeout> | undefined;
    const un = onClipsChanged(() => {
      clearTimeout(t);
      t = setTimeout(refreshAll, 120);
    });
    // Coming back from Android settings (or from another app): re-check everything.
    const onVisible = () => {
      if (document.visibilityState !== "visible") return;
      refreshStatus();
      checkPairLink();
      refreshAll();
    };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", onVisible);
    return () => {
      clearTimeout(t);
      un();
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", onVisible);
    };
  }, [refreshMeta, refreshStatus, checkPairLink, refreshAll]);

  // ---------- back gesture / Escape ----------
  const back = useEvent((): boolean => {
    if (openId) {
      setOpenId(null);
      return true;
    }
    if (settings) {
      setSettings(false);
      setPairLink(null);
      refreshStatus();
      return true;
    }
    if (search) {
      setSearch("");
      searchRef.current?.blur();
      return true;
    }
    if (filter !== "all") {
      setFilter("all");
      return true;
    }
    return false;
  });

  useEffect(() => {
    let un: (() => void) | undefined;
    let cancelled = false;
    if (isTauri && isAndroid) {
      void import("@tauri-apps/api/app").then(({ onBackButtonPress }) =>
        onBackButtonPress(() => {
          if (!back()) void androidApi.minimize().catch(() => undefined);
        }).then((l) => {
          if (cancelled) void l.unregister();
          else un = () => void l.unregister();
        }),
      );
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && back()) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      cancelled = true;
      un?.();
      window.removeEventListener("keydown", onKey);
    };
  }, [back]);

  // ---------- actions ----------
  const copy = useEvent((id: string, plain: boolean) => {
    api
      .copyClip(id, plain)
      .then(() => toast(plain ? "Copied as plain text" : "Copied"))
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
        if (filter === "pinned") void clips.refresh();
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
    setPending((p) => {
      const n = new Set(p);
      n.delete(id);
      return n;
    });
  });

  const remove = useEvent((c: ClipSummary) => {
    setOpenId(null);
    setPending((p) => new Set(p).add(c.id));
    const t = setTimeout(() => {
      timers.current.delete(c.id);
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
    toast({ message: "Clip deleted", action: { label: "Undo", run: () => undoDelete(c.id) }, duration: UNDO_MS });
  });

  const saveNow = () => {
    androidApi
      .saveClipboard()
      .then((r) => r.message && toast(r.message))
      .catch((e) => toast({ message: errorMessage(e), tone: "error" }));
  };

  const pick = (f: Filter) => {
    setFilter(f);
    timeline.current?.scrollToTop();
  };

  const select = useCallback((id: string) => setOpenId(id), []);
  const copyById = useCallback((id: string) => copy(id, false), [copy]);

  // ---------- derived ----------
  const byKind = new Map(stats?.by_kind ?? []);
  const chips: { f: Filter; label: string; count?: number; icon: React.ReactNode }[] = [
    { f: "all", label: "All", count: stats?.total, icon: <Layers size={14} strokeWidth={2} /> },
    { f: "pinned", label: "Pinned", count: stats?.pinned, icon: <Pin size={14} strokeWidth={2} /> },
    ...ALL_KINDS.filter((k) => (byKind.get(k) ?? 0) > 0 || filter === k).map((k) => {
      const Icon = KIND_META[k].icon;
      return {
        f: k as Filter,
        label: KIND_META[k].plural,
        count: byKind.get(k) ?? 0,
        icon: (
          <span className={`k-${k} k-fg`}>
            <Icon size={14} strokeWidth={2} />
          </span>
        ),
      };
    }),
  ];
  const setupNeeded = !!status && status.autoCapture && !(status.serviceEnabled && status.serviceConnected);
  const filtered = filter !== "all" || !!debounced;

  const top = (
    <div className="space-y-2 px-1 pt-1">
      {backendError && (
        <div className="flex items-start gap-2.5 rounded-2xl border border-[color-mix(in_oklab,var(--danger)_30%,transparent)] bg-[color-mix(in_oklab,var(--danger)_7%,transparent)] px-3.5 py-3 text-[13px] text-fg-2">
          <AlertTriangle size={16} className="mt-0.5 shrink-0 text-danger" />
          <span>
            <span className="font-medium text-fg">The clip store couldn't be opened.</span> {backendError}
          </span>
        </div>
      )}
      {setupNeeded && !filtered && (
        <button
          onClick={() => setSettings(true)}
          className="flex w-full items-center gap-3 rounded-2xl border border-line bg-panel px-3.5 py-3 text-left shadow-soft active:scale-[0.99]"
        >
          <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent-soft text-accent-text">
            <Zap size={19} strokeWidth={2} />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-[14px] font-semibold text-fg">Turn on automatic capture</span>
            <span className="block text-[12.5px] leading-[1.4] text-fg-3">
              Save everything you copy, without opening ClipTrail. Takes a minute.
            </span>
          </span>
          <ChevronRight size={18} className="shrink-0 text-fg-3" />
        </button>
      )}
    </div>
  );

  return (
    <div className="flex h-full flex-col bg-bg">
      <header className="shrink-0 px-4 pt-3">
        <div className="flex h-10 items-center gap-2.5">
          <img src="/logo.svg" alt="" className="h-[26px] w-[26px]" draggable={false} />
          <span className="text-[18px] font-semibold tracking-[-0.02em] text-fg">ClipTrail</span>
          <span className="flex-1" />
          <SyncChip onOpen={() => setSettings(true)} />
          <button
            aria-label="Settings"
            onClick={() => setSettings(true)}
            className="relative grid h-10 w-10 place-items-center rounded-xl text-fg-2 active:bg-active"
          >
            <Settings size={21} strokeWidth={1.9} />
            {setupNeeded && <span className="absolute top-2 right-2 h-2 w-2 rounded-full bg-accent" />}
          </button>
        </div>
        <label className="mt-2.5 flex h-11 items-center gap-2.5 rounded-xl border border-line bg-panel px-3.5 shadow-soft focus-within:border-[color-mix(in_oklab,var(--accent)_55%,transparent)] focus-within:shadow-[0_0_0_3px_var(--accent-soft)]">
          <Search size={17} strokeWidth={2} className="shrink-0 text-fg-3" />
          <input
            ref={searchRef}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && searchRef.current?.blur()}
            placeholder="Search your clips"
            enterKeyHint="search"
            spellCheck={false}
            autoComplete="off"
            className="h-full min-w-0 flex-1 bg-transparent text-[15px] text-fg outline-none"
          />
          {search && (
            <button
              aria-label="Clear search"
              onClick={() => setSearch("")}
              className="-mr-1.5 grid h-8 w-8 place-items-center rounded-full text-fg-3 active:bg-active"
            >
              <X size={16} strokeWidth={2.2} />
            </button>
          )}
        </label>
      </header>

      <nav className="no-scrollbar flex shrink-0 gap-2 overflow-x-auto px-4 pt-3 pb-2">
        {chips.map((c) => {
          const on = filter === c.f;
          return (
            <button
              key={c.f}
              onClick={() => pick(c.f)}
              className={cx(
                "inline-flex h-9 shrink-0 items-center gap-1.5 rounded-full border px-3.5 text-[13px] font-medium transition-colors",
                on ? "border-transparent bg-accent text-accent-fg" : "border-line bg-panel text-fg-2 active:bg-hover",
              )}
            >
              {!on && c.icon}
              {c.label}
              {c.count != null && c.count > 0 && (
                <span className={cx("tabular-nums", on ? "text-white/75" : "text-fg-3")}>{c.count.toLocaleString()}</span>
              )}
            </button>
          );
        })}
      </nav>

      <main className="min-h-0 flex-1">
        <Timeline
          ref={timeline}
          items={visible}
          loading={clips.loading}
          hasMore={clips.hasMore}
          filtered={filtered}
          shortcut=""
          mobile
          selectedId={openId}
          dayCounts={null}
          top={top}
          onLoadMore={clips.loadMore}
          onSelect={select}
          onCopy={copyById}
          onTogglePin={togglePin}
        />
      </main>

      {isAndroid && !openId && !settings && (
        <button
          aria-label="Save what's on the clipboard"
          onClick={saveNow}
          className="fixed right-5 bottom-5 z-30 grid h-14 w-14 place-items-center rounded-2xl bg-accent text-accent-fg shadow-[0_8px_24px_-6px_color-mix(in_oklab,var(--accent)_70%,transparent),inset_0_1px_0_rgb(255_255_255/0.18)] active:scale-95"
        >
          <ClipboardPlus size={23} strokeWidth={2} />
        </button>
      )}

      {open && <ClipSheet clip={open} onClose={() => setOpenId(null)} onCopy={copy} onTogglePin={togglePin} onDelete={remove} />}
      {settings && (
        <MobileSettings
          status={status}
          onStatus={setStatus}
          pairLink={pairLink}
          onPairLinkUsed={() => setPairLink(null)}
          onClose={() => {
            setSettings(false);
            setPairLink(null);
            refreshStatus();
          }}
        />
      )}
      <Toasts compact />
    </div>
  );
}

/** Header pill: sync state once the phone is paired. */
function SyncChip({ onOpen }: { onOpen: () => void }) {
  const v = useSyncView();
  if (!v || !v.status.configured || v.status.role !== "client") return null;
  const ok = v.enabled && v.status.connected;
  return (
    <button
      onClick={onOpen}
      className="inline-flex h-8 max-w-[150px] items-center gap-1.5 rounded-full border border-line bg-panel px-2.5 text-[12px] font-medium text-fg-2"
    >
      <span className={cx("h-[7px] w-[7px] shrink-0 rounded-full", ok ? "bg-success" : "bg-[#f59e0b]")} />
      <span className="truncate">{!v.enabled ? "Sync off" : ok ? (v.status.hub_name ?? "Synced") : "Offline"}</span>
    </button>
  );
}

/** Full-screen clip view: the desktop preview pane, with a back bar. */
function ClipSheet({
  clip,
  onClose,
  onCopy,
  onTogglePin,
  onDelete,
}: {
  clip: ClipSummary;
  onClose: () => void;
  onCopy: (id: string, plain: boolean) => void;
  onTogglePin: (c: ClipSummary) => void;
  onDelete: (c: ClipSummary) => void;
}) {
  // Synced images arrive as thumbnails; fetch the full image when opened.
  const [rev, setRev] = useState(0);
  useEffect(() => {
    if (clip.kind !== "image" || !isAndroid) return;
    let alive = true;
    void api
      .getClip(clip.id)
      .then((c) => {
        if (c && !c.image_path) {
          return syncApi.fetchImage(clip.id).then((ok) => alive && ok && setRev((r) => r + 1));
        }
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [clip.id, clip.kind]);

  return (
    <div className="anim-sheet fixed inset-0 z-40 flex flex-col bg-panel">
      <div className="flex h-12 shrink-0 items-center gap-1 border-b border-line px-1.5">
        <button
          aria-label="Back"
          onClick={onClose}
          className="grid h-10 w-10 place-items-center rounded-xl text-fg-2 active:bg-active"
        >
          <ArrowLeft size={21} strokeWidth={2} />
        </button>
        <span className="text-[15px] font-semibold text-fg">Clip</span>
      </div>
      <div className="min-h-0 flex-1">
        <Preview
          key={`${clip.id}:${rev}`}
          summary={clip}
          onCopy={onCopy}
          onTogglePin={onTogglePin}
          onDelete={onDelete}
          toast={(m) => toast(m)}
        />
      </div>
    </div>
  );
}
