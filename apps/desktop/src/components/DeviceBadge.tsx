import { platformIcon, useOtherDevice } from "../lib/sync";

/** Small "copied on <device>" pill for clips that came from another device. */
export function DeviceBadge({ deviceId }: { deviceId: string }) {
  const d = useOtherDevice(deviceId);
  if (!d) return null;
  const Icon = platformIcon(d.platform);
  return (
    <span
      title={`Copied on ${d.name}`}
      className="inline-flex h-[18px] max-w-[130px] shrink-0 items-center gap-1 rounded-[5px] bg-hover px-1.5 text-[10.5px] font-medium text-fg-2"
    >
      <Icon size={10.5} strokeWidth={2.2} className="shrink-0 text-fg-3" />
      <span className="truncate">{d.name}</span>
    </span>
  );
}

/** `<dt>Device</dt><dd>…</dd>` for the preview's metadata list (or nothing). */
export function DeviceMetaRow({ deviceId }: { deviceId: string }) {
  const d = useOtherDevice(deviceId);
  if (!d) return null;
  const Icon = platformIcon(d.platform);
  return (
    <>
      <dt className="text-fg-3">Device</dt>
      <dd className="min-w-0 truncate text-fg-2">
        <span className="inline-flex items-center gap-1.5">
          <Icon size={13} strokeWidth={2} className="shrink-0 text-fg-3" />
          {d.name}
        </span>
      </dd>
    </>
  );
}
