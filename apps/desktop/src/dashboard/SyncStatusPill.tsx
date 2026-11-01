import { MonitorSmartphone } from "lucide-react";
import { cx } from "../lib/cx";
import { otherDevices, useSyncView } from "../lib/sync";

/** Sidebar footer line: "Synced · 2 devices". Opens settings on click. */
export function SyncStatusPill({ onOpen }: { onOpen: () => void }) {
  const v = useSyncView();
  if (!v || !v.enabled) return null;
  const st = v.status;
  const devices = otherDevices(v);
  const online = devices.filter((d) => d.online).length;
  let tone: "ok" | "idle" | "warn" = "idle";
  let label: string;
  if (v.error) {
    tone = "warn";
    label = "Sync stopped · fix";
  } else if (!st.running) {
    label = "Sync starting…";
  } else if (st.role === "client") {
    tone = st.connected ? "ok" : "warn";
    label = st.connected ? `Synced with ${st.hub_name ?? "hub"}` : `${st.hub_name ?? "Hub"} offline`;
  } else if (devices.length === 0) {
    label = "Sync on · pair a phone";
  } else {
    tone = online > 0 ? "ok" : "idle";
    const n = devices.length;
    label = `${online > 0 ? "Synced" : "Sync on"} · ${n} device${n === 1 ? "" : "s"}`;
  }
  return (
    <button
      onClick={onOpen}
      title={
        st.role === "client"
          ? "Sync settings"
          : `${online} of ${devices.length} paired device${devices.length === 1 ? "" : "s"} online`
      }
      className={cx(
        "flex h-7 w-full items-center gap-2 rounded-lg px-2.5 text-[12px] font-medium transition-colors",
        tone === "warn"
          ? "text-[#b45309] hover:bg-[color-mix(in_oklab,#f59e0b_12%,transparent)] dark:text-[#fbbf24]"
          : "text-fg-2 hover:bg-hover",
      )}
    >
      <span className="relative grid h-3.5 w-3.5 shrink-0 place-items-center">
        <MonitorSmartphone size={13} strokeWidth={2} className={tone === "ok" ? "text-fg-2" : "text-fg-3"} />
        {tone === "ok" && (
          <span className="absolute -right-[3px] -bottom-[2px] h-[6px] w-[6px] rounded-full bg-success" />
        )}
      </span>
      <span className="flex-1 truncate text-left">{label}</span>
    </button>
  );
}
