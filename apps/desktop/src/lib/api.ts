// Thin API over the Tauri commands in src-tauri/src/commands.rs.
// Outside Tauri (plain `pnpm dev` in a browser) every call is served by an
// in-memory mock so the UI can be developed without the backend.

import type {
  CaptureState,
  Clip,
  ClipSummary,
  DayCount,
  IngestOutcome,
  ListQuery,
  Settings,
  SettingsView,
  Stats,
} from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type MockModule = typeof import("./mock");
let mockPromise: Promise<MockModule> | null = null;
function mock(): Promise<MockModule> {
  mockPromise ??= import("./mock");
  return mockPromise;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  const m = await mock();
  return m.handle(cmd, args ?? {}) as Promise<T>;
}

export const api = {
  listClips: (query: ListQuery) => call<ClipSummary[]>("list_clips", { query }),
  getClip: (id: string) => call<Clip | null>("get_clip", { id }),
  stats: () => call<Stats>("stats"),
  dayCounts: (from: number, to: number) => call<DayCount[]>("day_counts", { from, to }),
  sourceApps: () => call<string[]>("source_apps"),
  setPinned: (id: string, pinned: boolean) => call<void>("set_pinned", { id, pinned }),
  deleteClip: (id: string) => call<void>("delete_clip", { id }),
  copyClip: (id: string, plain: boolean) => call<void>("copy_clip", { id, plain }),
  /** Writes the clip, hides the picker and pastes into the previous app.
   *  Rejects with "paste-unavailable: ..." when paste-back isn't possible
   *  (e.g. Wayland); the clip is already on the clipboard then. */
  pasteClip: (id: string, plain: boolean) => call<void>("paste_clip", { id, plain }),
  hidePicker: () => call<void>("hide_picker"),
  showDashboard: () => call<void>("show_dashboard"),
  openUrl: (url: string) => call<void>("open_url", { url }),
  revealPath: (path: string) => call<void>("reveal_path", { path }),
  openDataDir: () => call<void>("open_data_dir"),
  getAutostart: () => call<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => call<void>("set_autostart", { enabled }),
  captureState: () => call<CaptureState>("capture_state"),
  /** minutes = null: until resumed. */
  pauseCapture: (minutes: number | null) => call<void>("pause_capture", { minutes }),
  resumeCapture: () => call<void>("resume_capture"),
  /** True when a native backdrop (Mica/acrylic) is applied to this window. */
  windowEffects: () => call<boolean>("window_effects"),
  setWindowTheme: (dark: boolean) => call<void>("set_window_theme", { dark }),
  backendStatus: () => call<string | null>("backend_status"),
  getSettings: () => call<SettingsView>("get_settings"),
  updateSettings: (settings: Settings) => call<SettingsView>("update_settings", { settings }),
};

export type Unlisten = () => void;

/** Subscribe to a backend event. Works in mock mode too. */
export function listen<T>(event: string, cb: (payload: T) => void): Unlisten {
  let un: Unlisten | null = null;
  let cancelled = false;
  if (isTauri) {
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<T>(event, (e) => cb(e.payload)).then((u) => {
        if (cancelled) u();
        else un = u;
      }),
    );
  } else {
    void mock().then((m) => {
      if (!cancelled) un = m.subscribe(event, cb as (p: unknown) => void);
    });
  }
  return () => {
    cancelled = true;
    un?.();
  };
}

/** Any change to the clip set: new copy, bump, pin, delete, prune. */
export function onClipsChanged(cb: (o: IngestOutcome | null) => void): Unlisten {
  const a = listen<IngestOutcome>("clips-changed", cb);
  const b = listen<null>("clips-updated", () => cb(null));
  return () => {
    a();
    b();
  };
}

/** Broadcast to every window (e.g. theme sync between dashboard and picker). */
export async function emitAll(event: string, payload: unknown): Promise<void> {
  if (isTauri) {
    const { emit } = await import("@tauri-apps/api/event");
    await emit(event, payload);
  } else {
    (await mock()).publish(event, payload);
  }
}

let convert: ((p: string) => string) | null = null;
if (isTauri) {
  void import("@tauri-apps/api/core").then((m) => {
    convert = (p) => m.convertFileSrc(p);
  });
}

/** URL for an image path on disk (asset protocol) or a mock data URL. */
export function assetUrl(path: string | null | undefined): string | undefined {
  if (!path) return undefined;
  if (path.startsWith("data:") || path.startsWith("http")) return path;
  if (convert) return convert(path);
  // Before the dynamic import settles; build the URL the way Tauri does.
  const enc = encodeURIComponent(path);
  return navigator.userAgent.includes("Windows")
    ? `http://asset.localhost/${enc}`
    : `asset://localhost/${enc}`;
}

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong";
}
