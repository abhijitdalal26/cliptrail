// Desktop auto-update: typed wrappers over the update_* commands in
// src-tauri/src/updater.rs plus one shared store for the banner and the
// Settings rows. Nothing here runs a check by itself: the backend checks in
// the background only when the user turned that on, and the buttons call
// `checkForUpdates`. Outside Tauri a small mock serves the calls.

import { useSyncExternalStore } from "react";
import { isTauri, listen } from "./api";

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
  date: string | null;
}

export interface CheckResult {
  status: "available" | "upToDate" | "failed";
  info: UpdateInfo | null;
  message: string | null;
}

export interface UpdateProgress {
  phase: "downloading" | "installing";
  downloaded: number;
  total: number | null;
}

/** Shown in "Release notes" (the tag is v<version>). */
export const RELEASES_URL = "https://github.com/abhijitdalal26/cliptrail/releases";
export const releaseNotesUrl = (version: string) => `${RELEASES_URL}/tag/v${version}`;

async function call<T>(cmd: string): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd);
  }
  return mockCall(cmd) as Promise<T>;
}

// ---- shared store ----

interface State {
  pending: UpdateInfo | null;
  progress: UpdateProgress | null;
  /** Install requested (download running or finishing). */
  installing: boolean;
  error: string | null;
}

let state: State = { pending: null, progress: null, installing: false, error: null };
const listeners = new Set<() => void>();
let started = false;

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

function subscribe(cb: () => void) {
  listeners.add(cb);
  if (!started) {
    started = true;
    void call<UpdateInfo | null>("update_pending")
      .then((p) => p && set({ pending: p }))
      .catch(() => undefined);
    listen<UpdateInfo>("update-available", (p) => set({ pending: p }));
    listen<UpdateProgress>("update-progress", (p) => set({ progress: p }));
  }
  return () => {
    listeners.delete(cb);
  };
}

export function useUpdates(): State {
  return useSyncExternalStore(subscribe, () => state);
}

/** Ask GitHub now (the manual button). Never installs anything. */
export async function checkForUpdates(): Promise<CheckResult> {
  try {
    const r = await call<CheckResult>("update_check");
    set({ pending: r.status === "available" ? r.info : null, error: null });
    return r;
  } catch {
    return { status: "failed", info: null, message: "Couldn't check for updates right now." };
  }
}

/** Download, verify and install the pending update, then restart. */
export async function installUpdate(): Promise<void> {
  set({ installing: true, error: null, progress: { phase: "downloading", downloaded: 0, total: null } });
  try {
    await call<void>("update_install");
  } catch (e) {
    set({
      installing: false,
      progress: null,
      error: typeof e === "string" ? e : "The update couldn't be installed. Please try again later.",
    });
  }
}

// ---- browser mock ----
// `?mockupdate` makes the mock find version 0.2.0; otherwise "up to date".

async function mockCall(cmd: string): Promise<unknown> {
  const available = new URLSearchParams(location.search).has("mockupdate");
  const info: UpdateInfo = {
    version: "0.2.0",
    currentVersion: "0.1.0",
    notes: "Clear history, optional update checks, fixes.",
    date: null,
  };
  switch (cmd) {
    case "update_pending":
      return null;
    case "update_check":
      await new Promise((r) => setTimeout(r, 900));
      return available
        ? ({ status: "available", info, message: null } satisfies CheckResult)
        : ({ status: "upToDate", info: null, message: null } satisfies CheckResult);
    case "update_install": {
      const m = await import("./mock");
      const total = 20_000_000;
      for (let d = 0; d <= total; d += 2_000_000) {
        m.publish("update-progress", { phase: "downloading", downloaded: d, total } satisfies UpdateProgress);
        await new Promise((r) => setTimeout(r, 250));
      }
      m.publish("update-progress", { phase: "installing", downloaded: 0, total: null } satisfies UpdateProgress);
      await new Promise((r) => setTimeout(r, 1200));
      throw "The browser preview can't install updates.";
    }
    default:
      return null;
  }
}
