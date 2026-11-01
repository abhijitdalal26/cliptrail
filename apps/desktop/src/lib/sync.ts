// Sync with paired devices: typed wrappers over the sync_* commands in
// src-tauri/src/sync.rs, plus one shared status store for the sidebar,
// clip badges and the Devices settings. Outside Tauri a small mock serves
// the calls so the UI can be developed in a browser.

import { useSyncExternalStore } from "react";
import { Laptop, Monitor, Smartphone, type LucideIcon } from "lucide-react";
import { isTauri, listen } from "./api";
import { isAndroid } from "./platform";
import type { Millis } from "./types";

export interface DeviceView {
  id: string;
  name: string;
  /** "windows" | "macos" | "linux" | "android" | "ios" */
  platform: string;
  paired_at: Millis;
  last_seen: Millis | null;
  revoked_at: Millis | null;
  is_self: boolean;
  online: boolean;
}

export interface SyncStatus {
  configured: boolean;
  role: "hub" | "client" | null;
  space_id: string | null;
  device_id: string;
  device_name: string | null;
  hub_name: string | null;
  running: boolean;
  connected: boolean;
  port: number | null;
  /** host:port the hub advertises (hub) or hub candidates (client). */
  hosts: string[];
  devices: DeviceView[];
  outbox: number;
  pending_images: number;
  last_seq: number;
  head_seq: number;
  last_error: string | null;
  keystore: string;
}

export interface SyncView {
  enabled: boolean;
  port: number;
  status: SyncStatus;
  /** Why sync isn't running although it's enabled. */
  error: string | null;
  /** device id → name */
  names: Record<string, string>;
}

export interface OfferView {
  uri: string;
  expires_at: Millis;
  hosts: string[];
  /** QR code as an SVG document. */
  svg: string;
}

/** Phone side of pairing (clipd_sync::PairOutcome). */
export interface PairOutcome {
  space_id: string;
  hub_name: string;
  hub_device_id: string;
  /** Same 4 digits the computer shows. */
  check_code: string;
  host: string;
  backfilled: number;
}

export interface PairedNotice {
  type: "paired";
  device_id: string;
  name: string;
  platform: string;
  check_code: string;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mockCall(cmd, args ?? {}) as Promise<T>;
}

export const syncApi = {
  status: () => call<SyncView>("sync_status"),
  setEnabled: (enabled: boolean) => call<SyncView>("sync_set_enabled", { enabled }),
  setPort: (port: number) => call<SyncView>("sync_set_port", { port }),
  restart: () => call<SyncView>("sync_restart"),
  createOffer: () => call<OfferView>("sync_create_offer"),
  cancelOffer: () => call<void>("sync_cancel_offer"),
  revoke: (deviceId: string) => call<void>("sync_revoke", { deviceId }),
  /** Phone: pair with the computer in a cliptrail://pair link. */
  pair: (uri: string) => call<PairOutcome>("sync_pair", { uri }),
  /** Phone: leave the computer's sync (clips stay). True when it confirmed. */
  unpair: () => call<boolean>("sync_unpair"),
  /** Phone: download a synced image's full size. */
  fetchImage: (id: string) => call<boolean>("sync_fetch_image", { id }),
};

// ---- shared store ----

let current: SyncView | null = null;
const listeners = new Set<() => void>();
let started = false;

export function setSyncView(v: SyncView) {
  current = v;
  listeners.forEach((l) => l());
}

export function refreshSync() {
  void syncApi
    .status()
    .then(setSyncView)
    .catch(() => undefined);
}

function subscribe(cb: () => void) {
  listeners.add(cb);
  if (!started) {
    started = true;
    refreshSync();
    listen("sync-changed", refreshSync);
    // Keeps "last seen" fresh.
    window.setInterval(refreshSync, 30_000);
  }
  return () => {
    listeners.delete(cb);
  };
}

/** Current sync state (null until the first load). */
export function useSyncView(): SyncView | null {
  return useSyncExternalStore(subscribe, () => current);
}

/** The other device a clip was last copied on; null for this device. */
export function useOtherDevice(deviceId: string): Pick<DeviceView, "name" | "platform"> | null {
  const v = useSyncView();
  if (!v || !v.status.configured || deviceId === v.status.device_id) return null;
  const d = v.status.devices.find((x) => x.id === deviceId);
  if (d) return { name: d.name, platform: d.platform };
  const name = v.names[deviceId];
  return name ? { name, platform: "" } : null;
}

export function platformIcon(platform: string): LucideIcon {
  if (platform === "android" || platform === "ios") return Smartphone;
  if (platform === "macos") return Laptop;
  return Monitor;
}

/** Paired devices other than this one (not revoked). */
export function otherDevices(v: SyncView): DeviceView[] {
  return v.status.devices.filter((d) => !d.is_self && d.revoked_at == null);
}

// ---- browser mock ----

const MOCK_SELF = "01MOCKSELF000000000000000";
const mock: { view: SyncView; offerCount: number } = {
  offerCount: 0,
  view: {
    enabled: true,
    port: 47821,
    error: null,
    names: {},
    status: {
      configured: true,
      role: "hub",
      space_id: "01MOCKSPACE",
      device_id: MOCK_SELF,
      device_name: "DESKTOP-MOCK",
      hub_name: "DESKTOP-MOCK",
      running: true,
      connected: true,
      port: 47821,
      hosts: ["192.168.1.23:47821"],
      devices: [
        {
          id: MOCK_SELF,
          name: "DESKTOP-MOCK",
          platform: "windows",
          paired_at: Date.now() - 86_400_000 * 9,
          last_seen: Date.now(),
          revoked_at: null,
          is_self: true,
          online: true,
        },
        {
          id: "01MOCKPHONE",
          name: "Galaxy S23 FE",
          platform: "android",
          paired_at: Date.now() - 86_400_000 * 3,
          last_seen: Date.now() - 60_000,
          revoked_at: null,
          is_self: false,
          online: true,
        },
      ],
      outbox: 0,
      pending_images: 0,
      last_seq: 42,
      head_seq: 42,
      last_error: null,
      keystore: "Windows Credential Manager",
    },
  },
};

function mockView(): SyncView {
  const v = mock.view;
  v.names = Object.fromEntries(v.status.devices.map((d) => [d.id, d.name]));
  v.status.running = v.enabled;
  return structuredClone(v);
}

function mockQr(seed: number): string {
  const n = 29;
  let rects = "";
  for (let y = 0; y < n; y++)
    for (let x = 0; x < n; x++) {
      const finder = (x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7);
      const on = finder
        ? !(x % 6 === 1 || y % 6 === 1) || (x % 6 >= 2 && x % 6 <= 4 && y % 6 >= 2 && y % 6 <= 4)
        : (Math.imul(x * 73 + y * 151 + seed * 31, 0x9e3779b1) >>> 13) % 2 === 0;
      if (on) rects += `<rect x="${x + 2}" y="${y + 2}" width="1" height="1"/>`;
    }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${n + 4} ${n + 4}"><rect width="100%" height="100%" fill="#fff"/><g fill="#000">${rects}</g></svg>`;
}

// The phone preview starts unpaired, as a fresh install does.
if (isAndroid) {
  const self = mock.view.status.devices[1];
  mock.view.status = {
    ...mock.view.status,
    configured: false,
    role: null,
    space_id: null,
    device_id: self.id,
    device_name: null,
    hub_name: null,
    connected: false,
    port: null,
    hosts: [],
    devices: [],
    keystore: "file (no-backup storage)",
  };
}

async function mockCall(cmd: string, a: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "sync_pair": {
      await new Promise((r) => setTimeout(r, 700));
      if (!String(a.uri).startsWith("cliptrail://pair")) throw "That isn't a ClipTrail pairing link";
      const st = mock.view.status;
      const now = Date.now();
      mock.view.status = {
        ...st,
        configured: true,
        role: "client",
        space_id: "01MOCKSPACE",
        device_name: "Galaxy S23 FE",
        hub_name: "DESKTOP-MOCK",
        connected: true,
        hosts: ["192.168.1.23:47821"],
        devices: [
          { id: MOCK_SELF, name: "DESKTOP-MOCK", platform: "windows", paired_at: now - 86_400_000 * 9, last_seen: now, revoked_at: null, is_self: false, online: true },
          { id: st.device_id, name: "Galaxy S23 FE", platform: "android", paired_at: now, last_seen: now, revoked_at: null, is_self: true, online: true },
        ],
      };
      return {
        space_id: "01MOCKSPACE",
        hub_name: "DESKTOP-MOCK",
        hub_device_id: MOCK_SELF,
        check_code: "4821",
        host: "192.168.1.23:47821",
        backfilled: 12,
      } satisfies PairOutcome;
    }
    case "sync_unpair":
      mock.view.status = { ...mock.view.status, configured: false, role: null, hub_name: null, connected: false, devices: [], hosts: [] };
      return true;
    case "sync_fetch_image":
      return false;
    case "sync_status":
    case "sync_restart":
      return mockView();
    case "sync_set_enabled":
      mock.view.enabled = Boolean(a.enabled);
      return mockView();
    case "sync_set_port":
      mock.view.port = Number(a.port);
      mock.view.status.port = Number(a.port);
      mock.view.status.hosts = [`192.168.1.23:${a.port}`];
      return mockView();
    case "sync_create_offer":
      mock.offerCount++;
      return {
        uri: `cliptrail://pair?v=1&h=192.168.1.23:${mock.view.port}&k=mockKey${mock.offerCount}&s=mockSecret&n=DESKTOP-MOCK`,
        expires_at: Date.now() + 10 * 60_000,
        hosts: mock.view.status.hosts,
        svg: mockQr(mock.offerCount),
      } satisfies OfferView;
    case "sync_cancel_offer":
      return null;
    case "sync_revoke":
      mock.view.status.devices = mock.view.status.devices.map((d) =>
        d.id === a.deviceId ? { ...d, revoked_at: Date.now(), online: false } : d,
      );
      return null;
    default:
      return null;
  }
}
