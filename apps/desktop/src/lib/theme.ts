import { useSyncExternalStore } from "react";
import { api, emitAll, isTauri, listen } from "./api";
import type { ThemePref } from "./types";

const KEY = "cliptrail.theme";
const media = window.matchMedia("(prefers-color-scheme: dark)");

let pref: ThemePref = readPref();
const subs = new Set<() => void>();

function readPref(): ThemePref {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    /* storage unavailable */
  }
  return "system";
}

export function resolveDark(p: ThemePref): boolean {
  return p === "dark" || (p === "system" && media.matches);
}

function apply(next: ThemePref, informNative: boolean) {
  pref = next;
  try {
    localStorage.setItem(KEY, next);
  } catch {
    /* ignore */
  }
  const dark = resolveDark(next);
  document.documentElement.classList.toggle("dark", dark);
  if (informNative && isTauri) void api.setWindowTheme(dark).catch(() => undefined);
  subs.forEach((f) => f());
}

/** Set the theme in this window, sync the other window, persist in settings. */
export function setThemePref(next: ThemePref, persist = true) {
  apply(next, true);
  void emitAll("theme-changed", next);
  if (persist) {
    void api
      .getSettings()
      .then((v) => (v.settings.theme === next ? v : api.updateSettings({ ...v.settings, theme: next })))
      .catch(() => undefined);
  }
}

let started = false;
/** Call once per window at boot. */
export function initTheme() {
  if (started) return;
  started = true;
  apply(pref, false);
  media.addEventListener("change", () => {
    if (pref === "system") apply("system", true);
  });
  listen<ThemePref>("theme-changed", (p) => {
    if (p !== pref) apply(p, false);
  });
  // The backend's settings.json is the source of truth.
  void api
    .getSettings()
    .then((v) => {
      if (v.settings.theme !== pref) apply(v.settings.theme, true);
    })
    .catch(() => undefined);
}

export function useTheme(): { pref: ThemePref; dark: boolean } {
  const p = useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => pref,
  );
  return { pref: p, dark: resolveDark(p) };
}
