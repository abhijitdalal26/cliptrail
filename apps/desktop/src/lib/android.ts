// Android-only commands (src-tauri/src/mobile/commands.rs → ClipTrailPlugin.kt).
// In the browser preview they are served by a small mock.

import { isTauri } from "./api";

export interface AndroidStatus {
  /** "Automatic capture (beta)" toggle. */
  autoCapture: boolean;
  /** Accessibility service switched on in Settings. */
  serviceEnabled: boolean;
  /** …and actually bound (on but not connected = toggle it off and on). */
  serviceConnected: boolean;
  notificationsAllowed: boolean;
  batteryUnrestricted: boolean;
  keepAliveRunning: boolean;
  overlayAllowed: boolean;
  /** Android 13+: sideloaded apps need "Allow restricted settings". */
  restrictedSettings: boolean;
  samsung: boolean;
  sdk: number;
  device: string;
  lastCaptureAt: number;
  lastSignal: string | null;
  autoCaptures: number;
  manualCaptures: number;
  misses: number;
}

export type SettingsScreen =
  | "accessibility"
  | "appInfo"
  | "battery"
  | "batteryOptimization"
  | "notifications"
  | "overlay";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mockCall(cmd, args ?? {}) as Promise<T>;
}

export const androidApi = {
  status: () => call<AndroidStatus>("android_status"),
  setAutoCapture: (enabled: boolean) => call<AndroidStatus>("android_set_auto_capture", { enabled }),
  open: (target: SettingsScreen) => call<unknown>("android_open", { target }),
  requestNotifications: () => call<{ granted: boolean }>("android_request_notifications"),
  /** `{ text }` or `{ cancelled: true }`. */
  scanQr: () => call<{ text?: string; cancelled?: boolean }>("android_scan_qr"),
  minimize: () => call<void>("android_minimize"),
  /** Read the clipboard now and save it; `message` is for a toast. */
  saveClipboard: () => call<{ message: string }>("android_save_clipboard"),
  /** A cliptrail://pair link that opened the app, once. */
  takePairLink: () => call<{ link: string | null }>("android_take_pair_link"),
};

// ---- browser mock ----

const mock: AndroidStatus = {
  autoCapture: true,
  serviceEnabled: false,
  serviceConnected: false,
  notificationsAllowed: true,
  batteryUnrestricted: false,
  keepAliveRunning: true,
  overlayAllowed: false,
  restrictedSettings: true,
  samsung: true,
  sdk: 36,
  device: "Galaxy S23 FE",
  lastCaptureAt: Date.now() - 4 * 60_000,
  lastSignal: "QS_TILE",
  autoCaptures: 0,
  manualCaptures: 3,
  misses: 0,
};

async function mockCall(cmd: string, a: Record<string, unknown>): Promise<unknown> {
  await new Promise((r) => setTimeout(r, 30));
  switch (cmd) {
    case "android_status":
      return { ...mock };
    case "android_set_auto_capture":
      mock.autoCapture = Boolean(a.enabled);
      mock.keepAliveRunning = mock.autoCapture;
      return { ...mock };
    case "android_open":
      // Pretend the user did what the screen asks.
      if (a.target === "accessibility") mock.serviceEnabled = mock.serviceConnected = true;
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
    default:
      return null;
  }
}
