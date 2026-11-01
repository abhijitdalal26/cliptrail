import { memo, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ClipboardCheck, Layers, LayoutDashboard, Pin, Search } from "lucide-react";
import { api, assetUrl, errorMessage, isTauri, listen, onClipsChanged } from "../lib/api";
import { appName } from "../lib/apps";
import { cx } from "../lib/cx";
import { parseColor, parseUrl, shortTime, splitPath } from "../lib/format";
import { isMac, MOD, useDebounced, useEvent, useTick } from "../lib/hooks";
import { KIND_META, subkindLabel } from "../lib/kinds";
import { toast } from "../lib/toast";
import type { ClipSummary, Kind, ListQuery } from "../lib/types";
import { useClips } from "../lib/useClips";
import { Kbd, KindTile } from "../components/ui";
import { Toasts } from "../components/Toasts";

type Tab = { id: string; label: string; kind?: Kind; pinned?: boolean };

const TABS: Tab[] = [
  { id: "all", label: "All" },
  { id: "pinned", label: "Pinned", pinned: true },
  { id: "link", label: "Links", kind: "link" },
  { id: "code", label: "Code", kind: "code" },
  { id: "text", label: "Text", kind: "text" },
  { id: "image", label: "Images", kind: "image" },
  { id: "file", label: "Files", kind: "file" },
  { id: "color", label: "Colors", kind: "color" },
  { id: "contact", label: "Contacts", kind: "contact" },
  { id: "sensitive", label: "Sensitive", kind: "sensitive" },
];

const ROW_H = 46;

export function Picker() {
  useTick(60_000);
  const [search, setSearch] = useState("");
  const debounced = useDebounced(search.trim(), 90);
  const [tab, setTab] = useState(0);
  const [sel, setSel] = useState(0);
  const [mode, setMode] = useState<"center" | "flyout">("center");
  const [openCount, setOpenCount] = useState(0);
  const [hint, setHint] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const tabsRef = useRef<HTMLDivElement>(null);

  const t = TABS[tab];
  const query = useMemo<ListQuery>(
    () => ({ kind: t.kind ?? null, pinned_only: !!t.pinned, search: debounced || null }),
    [t, debounced],
  );
  const clips = useClips(query, 60);
  const items = clips.items;
  const selIndex = Math.min(sel, Math.max(0, items.length - 1));
  const selected = items[selIndex] ?? null;

  useEffect(() => {
    if (isTauri) {
      void api
        .windowEffects()
        .then((on) => document.documentElement.classList.toggle("vibrant", on))
        .catch(() => undefined);
    }
    inputRef.current?.focus();
  }, []);

  const onShown = useEvent((m: { mode: "center" | "flyout" }) => {
    setMode(m.mode);
    setSearch("");
    setTab(0);
    setSel(0);
    setHint(null);
    setOpenCount((n) => n + 1);
    void clips.refresh();
    requestAnimationFrame(() => inputRef.current?.focus());
  });

  const refresh = clips.refresh;
  useEffect(() => {
    const a = listen("picker-shown", onShown);
    const b = onClipsChanged(() => void refresh());
    const focus = () => inputRef.current?.focus();
    window.addEventListener("focus", focus);
    return () => {
      a();
      b();
      window.removeEventListener("focus", focus);
    };
  }, [onShown, refresh]);

  const v = useVirtualizer({
    count: items.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => ROW_H,
    overscan: 6,
  });

  const go = (i: number) => {
    const n = Math.max(0, Math.min(items.length - 1, i));
    setSel(n);
    v.scrollToIndex(n, { align: "auto" });
    if (n >= items.length - 5) void clips.loadMore();
  };

  const switchTab = (i: number) => {
    const n = (i + TABS.length) % TABS.length;
    setTab(n);
    setSel(0);
    v.scrollToOffset(0);
    tabsRef.current?.children[n]?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
  };

  const paste = useEvent(async (c: ClipSummary | null, plain: boolean) => {
    if (!c) return;
    try {
      await api.pasteClip(c.id, plain);
    } catch {
      // Paste-back unavailable (e.g. Wayland, no previous window): fall back
      // to copying and tell the user to paste themselves.
      try {
        await api.copyClip(c.id, plain);
        setHint(`Copied — press ${MOD}+V to paste`);
        setTimeout(() => void api.hidePicker(), 1300);
      } catch (e) {
        toast({ message: errorMessage(e), tone: "error" });
      }
    }
  });

  const onKey = (e: React.KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      go(selIndex + 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      go(selIndex - 1);
    } else if (e.key === "PageDown") {
      e.preventDefault();
      go(selIndex + 7);
    } else if (e.key === "PageUp") {
      e.preventDefault();
      go(selIndex - 7);
    } else if (e.key === "Enter") {
      e.preventDefault();
      void paste(selected, e.shiftKey);
    } else if (e.key === "Tab") {
      e.preventDefault();
      switchTab(tab + (e.shiftKey ? -1 : 1));
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (search) setSearch("");
      else void api.hidePicker();
    } else if (mod && /^[1-9]$/.test(e.key)) {
      e.preventDefault();
      void paste(items[Number(e.key) - 1] ?? null, e.shiftKey);
    } else if (mod && e.key.toLowerCase() === "p" && selected) {
      e.preventDefault();
      const pinned = !selected.pinned;
      clips.patch(selected.id, { pinned });
      void api.setPinned(selected.id, pinned).catch(() => clips.patch(selected.id, { pinned: !pinned }));
    } else if (mod && e.key.toLowerCase() === "d") {
      e.preventDefault();
      void api.showDashboard();
    }
  };

  const body = (
    <div
      key={openCount}
      onKeyDown={onKey}
      className={cx(
        "flex h-full w-full flex-col overflow-hidden bg-panel",
        mode === "flyout" ? "anim-slide-up" : "anim-pop",
        !isTauri && "rounded-[14px] border border-line shadow-pop",
      )}
    >
      {/* Search */}
      <div data-tauri-drag-region className="flex h-[54px] shrink-0 items-center gap-3 px-4">
        <Search size={17} strokeWidth={2} className="shrink-0 text-fg-3" />
        <input
          ref={inputRef}
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            setSel(0);
          }}
          autoFocus
          spellCheck={false}
          autoComplete="off"
          placeholder="Search your clipboard…"
          className="h-full min-w-0 flex-1 bg-transparent text-[15px] text-fg outline-none placeholder:text-fg-3"
        />
        <button
          onClick={() => void api.showDashboard()}
          title={`Open dashboard (${MOD}+D)`}
          className="grid h-7 w-7 place-items-center rounded-lg text-fg-3 transition-colors hover:bg-hover hover:text-fg"
        >
          <LayoutDashboard size={15} strokeWidth={1.9} />
        </button>
      </div>

      {/* Kind chips */}
      <div
        ref={tabsRef}
        className="no-scrollbar flex shrink-0 gap-1 overflow-x-auto border-b border-line px-3 pb-2.5 [mask-image:linear-gradient(to_right,#000_92%,transparent)]"
      >
        {TABS.map((tb, i) => {
          const Icon = tb.kind ? KIND_META[tb.kind].icon : tb.pinned ? Pin : Layers;
          const on = i === tab;
          return (
            <button
              key={tb.id}
              tabIndex={-1}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => switchTab(i)}
              className={cx(
                "inline-flex h-[26px] shrink-0 items-center gap-1.5 rounded-full px-2.5 text-[12px] font-medium transition-colors duration-150",
                on ? "bg-accent text-accent-fg shadow-soft" : "text-fg-2 hover:bg-hover hover:text-fg",
              )}
            >
              <Icon size={12.5} strokeWidth={2.1} className={cx(!on && tb.kind && `k-${tb.kind} k-fg`)} />
              {tb.label}
            </button>
          );
        })}
      </div>

      {/* List */}
      <div ref={listRef} className="scroll-y min-h-0 flex-1 px-2 py-1.5" role="listbox">
        {items.length === 0 && !clips.loading ? (
          <div className="grid h-full place-items-center pb-6 text-center">
            <div>
              <div className="text-[13px] font-medium text-fg-2">
                {debounced ? "No matches" : t.id === "all" ? "Nothing copied yet" : `No ${t.label.toLowerCase()} yet`}
              </div>
              <div className="mt-1 text-[12px] text-fg-3">
                {debounced ? "Try fewer letters, or press Tab to switch kind." : "Copy something and it shows up here."}
              </div>
            </div>
          </div>
        ) : (
          <div className="relative" style={{ height: v.getTotalSize() }}>
            {v.getVirtualItems().map((vi) => {
              const c = items[vi.index];
              return (
                <div
                  key={c.id}
                  className="absolute top-0 left-0 w-full"
                  style={{ height: ROW_H, transform: `translateY(${vi.start}px)` }}
                >
                  <PickerRow
                    clip={c}
                    index={vi.index}
                    selected={vi.index === selIndex}
                    onHover={setSel}
                    onPick={(i, plain) => void paste(items[i], plain)}
                  />
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="flex h-[36px] shrink-0 items-center gap-3.5 border-t border-line px-4 text-[11.5px] text-fg-3">
        {hint ? (
          <span className="anim-fade flex items-center gap-1.5 font-medium text-success">
            <ClipboardCheck size={14} />
            {hint}
          </span>
        ) : (
          <>
            <Hint keys={["↵"]} label="Paste" />
            <Hint keys={["⇧", "↵"]} label="Plain text" />
            <Hint keys={[isMac ? "⌘" : "Ctrl", "1–9"]} label="Quick paste" />
            <Hint keys={["Tab"]} label="Kind" />
            <span className="flex-1" />
            <Hint keys={["Esc"]} label="Close" />
          </>
        )}
      </div>
      <Toasts compact />
    </div>
  );

  if (isTauri) return body;
  // Browser preview: show the panel floating over a wallpaper.
  return (
    <div className="picker-wallpaper grid h-full place-items-center">
      <div className="h-[460px] w-[640px]">{body}</div>
    </div>
  );
}

function Hint({ keys, label }: { keys: string[]; label: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <span className="flex gap-0.5">
        {keys.map((k) => (
          <Kbd key={k}>{k}</Kbd>
        ))}
      </span>
      {label}
    </span>
  );
}

const PickerRow = memo(function PickerRow({
  clip,
  index,
  selected,
  onHover,
  onPick,
}: {
  clip: ClipSummary;
  index: number;
  selected: boolean;
  onHover: (i: number) => void;
  onPick: (i: number, plain: boolean) => void;
}) {
  return (
    <div
      role="option"
      aria-selected={selected}
      onMouseMove={() => !selected && onHover(index)}
      onMouseDown={(e) => e.preventDefault()}
      onClick={(e) => onPick(index, e.shiftKey)}
      className={cx(
        "flex h-[44px] items-center gap-3 rounded-[10px] px-2.5 transition-colors duration-100",
        selected ? "bg-accent-soft" : "",
      )}
    >
      <RowLead clip={clip} />
      <div className="min-w-0 flex-1">
        <RowText clip={clip} />
      </div>
      <div className="flex shrink-0 items-center gap-2 text-[11.5px] text-fg-3">
        {clip.pinned && <Pin size={11} strokeWidth={2.4} className="fill-current text-accent-text" />}
        <span className="max-w-[110px] truncate">{appName(clip.source_app)}</span>
        <span className="w-[60px] text-right whitespace-nowrap tabular-nums">{shortTime(clip.last_seen_at)}</span>
        {index < 9 ? (
          <Kbd className={cx("w-[38px] transition-opacity", selected ? "opacity-100" : "opacity-60")}>
            {MOD === "Ctrl" ? `Ctrl ${index + 1}` : `⌘${index + 1}`}
          </Kbd>
        ) : (
          <span className="w-[38px]" />
        )}
      </div>
    </div>
  );
});

function RowLead({ clip }: { clip: ClipSummary }) {
  if (clip.kind === "image" && clip.thumb_path) {
    return (
      <img
        src={assetUrl(clip.thumb_path)}
        alt=""
        draggable={false}
        className="checker h-[28px] w-[40px] shrink-0 rounded-md object-cover shadow-[0_0_0_1px_var(--border)]"
      />
    );
  }
  if (clip.kind === "color") {
    const c = parseColor(clip.preview);
    return (
      <span
        className="h-[26px] w-[26px] shrink-0 rounded-[8px] shadow-[inset_0_0_0_1px_var(--border)]"
        style={{ background: c?.css }}
      />
    );
  }
  return <KindTile kind={clip.kind} size={26} />;
}

function RowText({ clip }: { clip: ClipSummary }) {
  const base = "truncate text-[13px] text-fg";
  switch (clip.kind) {
    case "link": {
      const u = parseUrl(clip.preview);
      return (
        <div className={base}>
          <span className="font-medium">{u?.host ?? clip.preview}</span>
          {u?.path && <span className="text-fg-3">{u.path}</span>}
        </div>
      );
    }
    case "code": {
      // First line that says something (skip lone braces / brackets).
      const lines = clip.preview.split("\n").map((l) => l.trim());
      const line = lines.find((l) => /[\p{L}\p{N}]/u.test(l)) ?? lines.find(Boolean) ?? "";
      return (
        <div className="flex min-w-0 items-center gap-2">
          <span className="code min-w-0 truncate text-[12.5px] text-fg">{line.trim()}</span>
          {clip.subkind && (
            <span className="k-code k-tile shrink-0 rounded px-1.5 text-[10px] leading-4 font-semibold">
              {subkindLabel("code", clip.subkind)}
            </span>
          )}
        </div>
      );
    }
    case "image":
      return (
        <div className={base}>
          {clip.subkind === "screenshot" ? "Screenshot" : "Image"}
          {clip.width && clip.height && <span className="text-fg-3"> · {clip.width}×{clip.height}</span>}
        </div>
      );
    case "file": {
      const { name, dir } = splitPath(clip.files[0] ?? clip.preview);
      return (
        <div className={base}>
          <span className="font-medium">{name}</span>
          {clip.files.length > 1 && <span className="text-fg-3"> +{clip.files.length - 1}</span>}
          <span className="text-fg-3"> · {dir}</span>
        </div>
      );
    }
    case "sensitive":
      return (
        <div className={base}>
          <span className="tracking-[0.15em] text-fg-2">••••••••</span>
          <span className="text-fg-3"> · {subkindLabel("sensitive", clip.subkind) ?? "Sensitive"}</span>
        </div>
      );
    case "color":
      return <div className={cx(base, "code text-[12.5px]")}>{clip.preview}</div>;
    default:
      return <div className={base}>{clip.preview.replace(/\s+/g, " ").trim()}</div>;
  }
}
