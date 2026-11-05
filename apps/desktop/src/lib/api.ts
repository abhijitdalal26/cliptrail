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
  Transform,
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
  /** How many clips "Clear history" would delete. */
  clearCount: (keepPinned: boolean) => call<number>("clear_count", { keepPinned }),
  /** Delete the history (and image files). By default this device only;
   *  `everywhere` also deletes the clips on paired devices. Returns the count. */
  clearHistory: (keepPinned: boolean, everywhere: boolean) =>
    call<number>("clear_history", { keepPinned, everywhere }),
  /** With `transform`, the transformed text is written as plain text. */
  copyClip: (id: string, plain: boolean, transform?: Transform) =>
    call<void>("copy_clip", { id, plain, transform: transform ?? null }),
  /** Writes the clip, hides the picker and pastes into the previous app.
   *  Rejects with "paste-unavailable: ..." when paste-back isn't possible
   *  (e.g. Wayland); the clip is already on the clipboard then. */
  pasteClip: (id: string, plain: boolean, transform?: Transform) =>
    call<void>("paste_clip", { id, plain, transform: transform ?? null }),
  /** The clip's text run through a transform (for previews). */
  transformClip: (id: string, transform: Transform) => call<string>("transform_clip", { id, transform }),
  /** Transforms that make sense for this clip and would change it. */
  clipActions: (id: string) => call<Transform[]>("clip_actions", { id }),
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
  /** macOS: whether paste-back may post synthetic input (Accessibility). */
  accessibilityState: () => call<boolean>("accessibility_state"),
  /** macOS: deep-link to System Settings → Privacy & Security → Accessibility. */
  openAccessibilitySettings: () => call<void>("open_accessibility_settings"),
  /** True when a native backdrop (Mica/acrylic) is applied to this window. */
  windowEffects: () => call<boolean>("window_effects"),
  /** `panel` (Android): the bars sit over a panel-coloured screen (the clip view). Ignored on desktop. */
  setWindowTheme: (dark: boolean, panel = false) => call<void>("set_window_theme", { dark, panel }),
  /** Release the global hotkeys while Settings records a new one (desktop). */
  suspendHotkeys: (suspended: boolean) => call<void>("suspend_hotkeys", { suspended }),
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

/** Stored images are encrypted on disk; the app's `clipimg` protocol
 * (src-tauri/src/imageproto.rs) decrypts them for the webview. */
const IMAGE_SCHEME = "clipimg";

let convert: ((p: string) => string) | null = null;
if (isTauri) {
  void import("@tauri-apps/api/core").then((m) => {
    convert = (p) => m.convertFileSrc(p, IMAGE_SCHEME);
  });
}

/** URL for a stored image or thumbnail path, or a mock data URL. */
export function assetUrl(path: string | null | undefined): string | undefined {
  if (!path) return undefined;
  if (path.startsWith("data:") || path.startsWith("http")) return path;
  if (convert) return convert(path);
  // Before the dynamic import settles; build the URL the way Tauri does.
  const enc = encodeURIComponent(path);
  return /Windows|Android/.test(navigator.userAgent)
    ? `http://${IMAGE_SCHEME}.localhost/${enc}`
    : `${IMAGE_SCHEME}://localhost/${enc}`;
}

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong";
}
