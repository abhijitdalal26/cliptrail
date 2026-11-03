import { useEffect, useState, type ReactNode } from "react";
import { formatDistanceToNowStrict } from "date-fns";
import { CircleCheck, Copy, QrCode, RefreshCw, ShieldCheck, TriangleAlert, Wifi } from "lucide-react";
import { errorMessage, listen } from "../lib/api";
import { cx } from "../lib/cx";
import {
  otherDevices,
  platformIcon,
  refreshSync,
  setSyncView,
  syncApi,
  useSyncView,
  type DeviceView,
  type OfferView,
  type PairedNotice,
  type SyncView,
} from "../lib/sync";
import type { Millis } from "../lib/types";
import { RelayCard } from "../components/RelayCard";
import { Button, Switch } from "../components/ui";

function ago(t: Millis | null): string {
  if (!t) return "never";
  if (Date.now() - t < 45_000) return "just now";
  return `${formatDistanceToNowStrict(t)} ago`;
}

/** Settings › Devices: sync on/off, pair a phone, paired devices, port. */
export function DevicesSection() {
  const view = useSyncView();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => refreshSync(), []);

  if (!view) return null;
  const st = view.status;
  const isClient = st.role === "client";
  const devices = otherDevices(view);

  const run = async (f: () => Promise<SyncView | void>) => {
    setBusy(true);
    try {
      const v = await f();
      if (v) setSyncView(v);
      else refreshSync();
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section>
      <h3 className="mb-2 px-1 text-[11px] font-semibold tracking-[0.05em] text-fg-3 uppercase">Devices</h3>
      <div className="divide-y divide-[var(--border)] overflow-visible rounded-xl border border-line bg-panel">
        <Row
          title="Sync with my devices"
          desc={
            <>
              Clips you copy show up on your paired phone and computers, end-to-end encrypted. Works over your
              local network; nothing goes to the cloud.
              <StatusLine view={view} />
            </>
          }
          error={error ?? view.error}
        >
          <div className="flex items-center gap-2">
            {view.enabled && view.error && (
              <Button size="sm" icon={RefreshCw} disabled={busy} onClick={() => void run(syncApi.restart)}>
                Retry
              </Button>
            )}
            <Switch
              label="Sync with my devices"
              checked={view.enabled}
              disabled={busy}
              onChange={(v) => void run(() => syncApi.setEnabled(v))}
            />
          </div>
        </Row>

        {view.enabled && st.running && !isClient && <PairRow />}

        {view.enabled && devices.length > 0 && (
          <div className="px-4 py-2">
            {devices.map((d) => (
              <DeviceRow key={d.id} device={d} canRevoke={!isClient} onError={setError} />
            ))}
          </div>
        )}

        {view.enabled && st.configured && st.running && <RelayCard view={view} />}

        {view.enabled && !isClient && <PortRow view={view} busy={busy} run={run} />}
      </div>
    </section>
  );
}

function StatusLine({ view }: { view: SyncView }) {
  const st = view.status;
  if (!view.enabled) return null;
  let tone: "ok" | "idle" | "warn" = "idle";
  let text: string;
  if (view.error || !st.running) {
    tone = "warn";
    text = view.error ? "Not running" : "Starting…";
  } else if (st.role === "client") {
    tone = st.connected ? "ok" : "warn";
    text = st.connected
      ? `Connected to ${st.hub_name ?? "your hub"}`
      : `Waiting for ${st.hub_name ?? "your hub"}${st.last_error ? ` · ${st.last_error}` : ""}`;
  } else {
    const n = otherDevices(view).length;
    const online = otherDevices(view).filter((d) => d.online).length;
    tone = online > 0 ? "ok" : "idle";
    const where = st.hosts[0] ?? `port ${st.port ?? view.port}`;
    text =
      n === 0
        ? `Ready on ${where} · no devices paired yet`
        : `Ready on ${where} · ${online} of ${n} device${n === 1 ? "" : "s"} online`;
  }
  return (
    <span className="mt-1.5 flex items-center gap-1.5 text-[11.5px] font-medium">
      <span
        className={cx(
          "h-[7px] w-[7px] shrink-0 rounded-full",
          tone === "ok" && "bg-success",
          tone === "idle" && "bg-fg-4",
          tone === "warn" && "bg-[#f59e0b]",
        )}
      />
      <span className={cx("truncate", tone === "warn" ? "text-[#b45309] dark:text-[#fbbf24]" : "text-fg-2")}>
        {text}
      </span>
    </span>
  );
}

function PairRow() {
  const [offer, setOffer] = useState<OfferView | null>(null);
  const [paired, setPaired] = useState<PairedNotice | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!offer) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    const un = listen<PairedNotice>("sync-paired", (n) => {
      setPaired(n);
      setOffer(null);
      refreshSync();
    });
    return () => {
      window.clearInterval(t);
      un();
    };
  }, [offer]);

  useEffect(() => () => void syncApi.cancelOffer().catch(() => undefined), []);

  const start = async () => {
    try {
      setPaired(null);
      setCopied(false);
      const o = await syncApi.createOffer();
      setNow(Date.now());
      setOffer(o);
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  const close = () => {
    setOffer(null);
    void syncApi.cancelOffer().catch(() => undefined);
  };

  const left = offer ? Math.max(0, offer.expires_at - now) : 0;
  const expired = offer != null && left === 0;
  // No device after 20 s usually means the firewall or the Wi-Fi is in the way.
  const shownFor = offer ? now - (offer.expires_at - 10 * 60_000) : 0;
  const waitedLong = offer != null && !expired && shownFor > 20_000;

  return (
    <div className="px-4 py-3.5">
      <div className="flex items-center gap-6">
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-fg">Pair a phone</div>
          <div className="mt-0.5 text-[12px] leading-[1.45] text-fg-3">
            Open ClipTrail on your phone and scan the code. Both need to be on the same Wi-Fi.
          </div>
        </div>
        {!offer && (
          <Button size="sm" variant="primary" icon={QrCode} onClick={() => void start()}>
            {paired ? "Pair another" : "Pair a phone"}
          </Button>
        )}
      </div>
      {error && <div className="anim-fade mt-2 text-[12px] font-medium text-danger">{error}</div>}

      {paired && !offer && (
        <div className="anim-pop mt-3 flex items-center gap-3 rounded-xl border border-line bg-panel-2 px-3.5 py-3">
          <CircleCheck size={18} className="shrink-0 text-success" />
          <div className="min-w-0 flex-1 text-[12.5px]">
            <div className="font-medium text-fg">Paired with {paired.name}</div>
            <div className="text-fg-3">
              Check code <span className="font-semibold text-fg-2 tabular-nums">{paired.check_code}</span> should
              match the one on your phone. If it doesn't, revoke the device below.
            </div>
          </div>
        </div>
      )}

      {offer && (
        <div className="anim-pop mt-3 flex gap-4 rounded-xl border border-line bg-panel-2 p-3.5">
          <div
            className={cx(
              "h-[168px] w-[168px] shrink-0 overflow-hidden rounded-lg bg-white p-1.5 shadow-[0_0_0_1px_var(--border)] transition-opacity",
              "[&>svg]:h-full [&>svg]:w-full",
              expired && "opacity-25",
            )}
            // The SVG is generated by the backend's QR encoder from our own pairing link.
            dangerouslySetInnerHTML={{ __html: offer.svg }}
          />
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="text-[13px] font-medium text-fg">
              {expired ? "This code expired" : "Scan with the ClipTrail app"}
            </div>
            <div className="mt-0.5 text-[12px] text-fg-3 tabular-nums">
              {expired ? (
                "Make a new one to keep pairing."
              ) : (
                <>
                  Single use · expires in {Math.floor(left / 60_000)}:{String(Math.floor(left / 1000) % 60).padStart(2, "0")}
                </>
              )}
            </div>
            <div className="mt-2.5 flex items-center gap-1.5">
              <code className="code min-w-0 flex-1 truncate rounded-md border border-line bg-panel px-2 py-1 text-[11px] text-fg-2">
                {offer.uri}
              </code>
              <Button
                size="sm"
                variant="ghost"
                icon={copied ? CircleCheck : Copy}
                onClick={() =>
                  void navigator.clipboard
                    .writeText(offer.uri)
                    .then(() => setCopied(true))
                    .catch(() => undefined)
                }
              >
                {copied ? "Copied" : "Copy link"}
              </Button>
            </div>
            <div className="mt-2 flex items-start gap-1.5 text-[11.5px] leading-[1.45] text-fg-3">
              <Wifi size={12.5} className="mt-[1px] shrink-0" />
              <span>
                Phone connects to {offer.hosts.length ? offer.hosts.join(" or ") : "this PC"}.
                {waitedLong && (
                  <>
                    {" "}
                    Nothing yet? Check the phone is on the same Wi-Fi (guest networks often block this) and that
                    this network is set to <span className="font-medium text-fg-2">Private</span> in Windows Settings ›
                    Network &amp; internet.
                  </>
                )}
              </span>
            </div>
            <div className="mt-auto flex justify-end gap-1.5 pt-2.5">
              {expired && (
                <Button size="sm" icon={RefreshCw} onClick={() => void start()}>
                  New code
                </Button>
              )}
              <Button size="sm" onClick={close}>
                Done
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function DeviceRow({
  device: d,
  canRevoke,
  onError,
}: {
  device: DeviceView;
  canRevoke: boolean;
  onError: (m: string | null) => void;
}) {
  const [confirm, setConfirm] = useState(false);
  useEffect(() => {
    if (!confirm) return;
    const t = window.setTimeout(() => setConfirm(false), 3500);
    return () => window.clearTimeout(t);
  }, [confirm]);
  const Icon = platformIcon(d.platform);
  return (
    <div className="flex items-center gap-3 py-1.5">
      <span className="relative grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-hover text-fg-2">
        <Icon size={15} strokeWidth={1.9} />
        {d.online && (
          <span className="absolute -right-0.5 -bottom-0.5 h-2.5 w-2.5 rounded-full border-2 border-[var(--panel)] bg-success" />
        )}
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium text-fg">{d.name}</div>
        <div className="truncate text-[11.5px] text-fg-3">
          {d.online ? "Online now" : `Last seen ${ago(d.last_seen)}`} · paired {ago(d.paired_at)}
        </div>
      </div>
      {canRevoke && (
        <Button
          size="sm"
          variant={confirm ? "danger" : "ghost"}
          className={cx(confirm && "bg-[color-mix(in_oklab,var(--danger)_10%,transparent)]")}
          onClick={() => {
            if (!confirm) {
              setConfirm(true);
              return;
            }
            void syncApi
              .revoke(d.id)
              .then(() => {
                onError(null);
                refreshSync();
              })
              .catch((e) => onError(errorMessage(e)));
          }}
        >
          {confirm ? "Confirm revoke" : "Revoke"}
        </Button>
      )}
    </div>
  );
}

function PortRow({
  view,
  busy,
  run,
}: {
  view: SyncView;
  busy: boolean;
  run: (f: () => Promise<SyncView | void>) => Promise<void>;
}) {
  const [draft, setDraft] = useState(String(view.port));
  useEffect(() => setDraft(String(view.port)), [view.port]);
  const n = Number(draft);
  const valid = Number.isInteger(n) && n >= 1024 && n <= 65535;
  const dirty = draft !== String(view.port);
  return (
    <Row
      title="Network"
      desc={
        <span className="flex items-start gap-1.5">
          <ShieldCheck size={12.5} className="mt-[2px] shrink-0 text-success" />
          <span>
            Windows asks once to let ClipTrail through the firewall: allow it on{" "}
            <span className="font-medium text-fg-2">Private networks</span>. Paired devices only; every clip is
            encrypted before it leaves this PC. Keys are kept in {view.status.keystore || "the system keychain"}.
          </span>
        </span>
      }
    >
      <div className="flex items-center gap-1.5">
        <span className="text-[12px] text-fg-3">Port</span>
        <input
          value={draft}
          inputMode="numeric"
          onChange={(e) => setDraft(e.target.value.replace(/\D/g, "").slice(0, 5))}
          onKeyDown={(e) => e.key === "Enter" && valid && dirty && void run(() => syncApi.setPort(n))}
          className={cx(
            "code h-8 w-[72px] rounded-lg border bg-panel px-2 text-center text-[12.5px] text-fg shadow-soft outline-none",
            valid ? "border-line focus-visible:border-accent" : "border-danger",
          )}
        />
        {dirty && (
          <Button size="sm" disabled={!valid || busy} onClick={() => void run(() => syncApi.setPort(n))}>
            Apply
          </Button>
        )}
      </div>
    </Row>
  );
}

function Row({
  title,
  desc,
  error,
  children,
}: {
  title: string;
  desc?: ReactNode;
  error?: string | null;
  children: ReactNode;
}) {
  return (
    <div className="px-4 py-3.5">
      <div className="flex items-center gap-6">
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-fg">{title}</div>
          {desc && <div className="mt-0.5 text-[12px] leading-[1.45] text-fg-3">{desc}</div>}
        </div>
        <div className="shrink-0">{children}</div>
      </div>
      {error && (
        <div className="anim-fade mt-2 flex items-center gap-1.5 text-[12px] font-medium text-danger">
          <TriangleAlert size={12.5} className="shrink-0" />
          {error}
        </div>
      )}
    </div>
  );
}
