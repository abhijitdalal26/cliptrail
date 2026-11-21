// Android-only commands (src-tauri/src/mobile/commands.rs → ClipTrailPlugin.kt).
// In the browser preview they are served by a small mock.

import { isTauri } from "./api";

export interface AndroidStatus {
  /** "Keep running in the background": the notification with Save clipboard, and sync. */
  keepAlive: boolean;
  notificationsAllowed: boolean;
  batteryUnrestricted: boolean;
  keepAliveRunning: boolean;
  samsung: boolean;
  sdk: number;
  device: string;
  lastCaptureAt: number;
  lastSignal: string | null;
  /** Clips saved on this phone (tile, notification, share, app). */
  captures: number;
}

export type SettingsScreen = "appInfo" | "battery" | "batteryOptimization" | "notifications";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mockCall(cmd, args ?? {}) as Promise<T>;
}

export const androidApi = {
  status: () => call<AndroidStatus>("android_status"),
  setKeepAlive: (enabled: boolean) => call<AndroidStatus>("android_set_keep_alive", { enabled }),
  open: (target: SettingsScreen) => call<unknown>("android_open", { target }),
  requestNotifications: () => call<{ granted: boolean }>("android_request_notifications"),
  /** `{ text }` or `{ cancelled: true }`. */
  scanQr: () => call<{ text?: string; cancelled?: boolean }>("android_scan_qr"),
  minimize: () => call<void>("android_minimize"),
  /** Read the clipboard now and save it; `message` is for a toast. */
  saveClipboard: () => call<{ message: string }>("android_save_clipboard"),
  /** A cliptrail://pair link that opened the app, once. */
  takePairLink: () => call<{ link: string | null }>("android_take_pair_link"),
  /** Plain text on the clipboard ("" when none); the WebView can't read it itself. */
  readClipboard: () => call<{ text: string }>("android_read_clipboard"),
};

// ---- browser mock ----

const mock: AndroidStatus = {
  keepAlive: true,
  notificationsAllowed: false,
  batteryUnrestricted: false,
  keepAliveRunning: true,
  samsung: true,
  sdk: 36,
  device: "Galaxy S23 FE",
  lastCaptureAt: Date.now() - 4 * 60_000,
  lastSignal: "QS_TILE",
  captures: 3,
};

async function mockCall(cmd: string, a: Record<string, unknown>): Promise<unknown> {
  await new Promise((r) => setTimeout(r, 30));
  switch (cmd) {
    case "android_status":
      return { ...mock };
    case "android_set_keep_alive":
      mock.keepAlive = Boolean(a.enabled);
      mock.keepAliveRunning = mock.keepAlive;
      return { ...mock };
    case "android_open":
      // Pretend the user did what the screen asks.
      if (a.target === "batteryOptimization") mock.batteryUnrestricted = true;
      if (a.target === "notifications") mock.notificationsAllowed = true;
      return { opened: true };
    case "android_request_notifications":
      mock.notificationsAllowed = true;
      return { granted: true };
    case "android_scan_qr":
      throw "The camera isn't available in the browser preview; paste the link instead";
    case "android_save_clipboard":
      return { message: "Nothing to save" };
    case "android_take_pair_link":
      return { link: null };
    case "android_read_clipboard":
      return { text: await navigator.clipboard.readText() };
    default:
      return null;
  }
}
