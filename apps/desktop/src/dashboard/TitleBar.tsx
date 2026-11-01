import { forwardRef, useEffect, useState } from "react";
import { Minus, Monitor, Moon, Search, Square, Copy as Restore, Sun, X } from "lucide-react";
import { isTauri } from "../lib/api";
import { cx } from "../lib/cx";
import { setThemePref, useTheme } from "../lib/theme";
import type { ThemePref } from "../lib/types";
import { IconButton, Kbd } from "../components/ui";

interface Props {
  search: string;
  onSearch: (s: string) => void;
  onSearchKey: (e: React.KeyboardEvent<HTMLInputElement>) => void;
}

export const TitleBar = forwardRef<HTMLInputElement, Props>(function TitleBar({ search, onSearch, onSearchKey }, ref) {
  return (
    <header data-tauri-drag-region className="relative flex h-[52px] shrink-0 items-center">
      {/* Brand, aligned with the sidebar */}
      <div data-tauri-drag-region className="flex w-[232px] shrink-0 items-center gap-2.5 pl-5">
        <img src="/logo.svg" alt="" className="pointer-events-none h-[22px] w-[22px]" draggable={false} />
        <span data-tauri-drag-region className="text-[14px] font-semibold tracking-[-0.015em] text-fg">
          ClipTrail
        </span>
      </div>

      {/* Search */}
      <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center pr-3">
        <label
          className={cx(
            "group flex h-9 w-full max-w-[560px] items-center gap-2.5 rounded-[10px] border border-line bg-panel px-3 shadow-soft",
            "transition-[border-color,box-shadow] duration-150 focus-within:border-[color-mix(in_oklab,var(--accent)_55%,transparent)] focus-within:shadow-[0_0_0_3px_var(--accent-soft)]",
          )}
        >
          <Search size={15} strokeWidth={2} className="shrink-0 text-fg-3 group-focus-within:text-accent-text" />
          <input
            ref={ref}
            value={search}
            onChange={(e) => onSearch(e.target.value)}
            onKeyDown={onSearchKey}
            placeholder="Search everything you've copied…"
            spellCheck={false}
            autoComplete="off"
            className="h-full min-w-0 flex-1 bg-transparent text-[13px] text-fg outline-none"
          />
          {search ? (
            <button
              aria-label="Clear search"
              onClick={() => onSearch("")}
              className="grid h-5 w-5 place-items-center rounded-full text-fg-3 hover:bg-hover hover:text-fg"
            >
              <X size={13} strokeWidth={2.2} />
            </button>
          ) : (
            <Kbd>/</Kbd>
          )}
        </label>
      </div>

      <div className="flex items-center gap-1 pr-2">
        <ThemeToggle />
      </div>
      <WindowControls />
    </header>
  );
});

function ThemeToggle() {
  const { pref } = useTheme();
  const next: Record<ThemePref, ThemePref> = { system: "light", light: "dark", dark: "system" };
  const icon = pref === "system" ? Monitor : pref === "light" ? Sun : Moon;
  const label = pref === "system" ? "Theme: system" : pref === "light" ? "Theme: light" : "Theme: dark";
  return <IconButton icon={icon} label={label} onClick={() => setThemePref(next[pref])} />;
}

function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    let un: (() => void) | undefined;
    void import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
      const w = getCurrentWindow();
      setMaximized(await w.isMaximized());
      un = await w.onResized(async () => setMaximized(await w.isMaximized()));
    });
    return () => un?.();
  }, []);

  const act = (fn: "minimize" | "toggleMaximize" | "close") => {
    if (!isTauri) return;
    void import("@tauri-apps/api/window").then(({ getCurrentWindow }) => getCurrentWindow()[fn]());
  };

  const btn =
    "grid h-[52px] w-[46px] place-items-center text-fg-2 transition-colors duration-100 hover:bg-hover hover:text-fg";
  return (
    <div className="flex self-start">
      <button aria-label="Minimize" className={btn} onClick={() => act("minimize")}>
        <Minus size={15} strokeWidth={1.6} />
      </button>
      <button aria-label={maximized ? "Restore" : "Maximize"} className={btn} onClick={() => act("toggleMaximize")}>
        {maximized ? (
          <Restore size={12} strokeWidth={1.6} className="-scale-x-100" />
        ) : (
          <Square size={11.5} strokeWidth={1.6} />
        )}
      </button>
      <button
        aria-label="Close to tray"
        className={cx(btn, "hover:!bg-[#c42b1c] hover:!text-white")}
        onClick={() => act("close")}
      >
        <X size={16} strokeWidth={1.6} />
      </button>
    </div>
  );
}
