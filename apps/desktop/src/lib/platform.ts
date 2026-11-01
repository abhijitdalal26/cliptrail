// Which shell are we in? The same UI runs as the desktop app (Windows /
// Linux / macOS), as the Android app, and in a browser with the mock
// backend. In the browser, add `?platform=android` to preview the phone UI.

import { useSyncExternalStore } from "react";

const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : null;

/** Running as the Android app (or the browser mock asked to pretend). */
export const isAndroid =
  (typeof navigator !== "undefined" && /Android/i.test(navigator.userAgent)) || params?.get("platform") === "android";

/** Narrow windows get the phone layout too (handy for the browser preview). */
const narrow = typeof window !== "undefined" ? window.matchMedia("(max-width: 640px)") : null;

function subscribe(cb: () => void) {
  narrow?.addEventListener("change", cb);
  return () => narrow?.removeEventListener("change", cb);
}

/** True when the single-column phone layout should be used. */
export function useMobileLayout(): boolean {
  return useSyncExternalStore(subscribe, () => isAndroid || !!narrow?.matches);
}
