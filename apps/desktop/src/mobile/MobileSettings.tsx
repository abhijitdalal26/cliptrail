import { useEffect, useRef, useState } from "react";
import { formatDistanceToNowStrict } from "date-fns";
import {
  ArrowLeft,
  BatteryCharging,
  Bell,
  CircleCheck,
  ClipboardPaste,
  Hand,
  Link2,
  LockOpen,
  Monitor,
  Moon,
  QrCode,
  ShieldCheck,
  Sun,
  Trash2,
  Unplug,
} from "lucide-react";
import { api, errorMessage, listen } from "../lib/api";
import { androidApi, type AndroidStatus, type SettingsScreen } from "../lib/android";
import { cx } from "../lib/cx";
import { untilLabel } from "../lib/format";
import { isAndroid } from "../lib/platform";
import { otherDevices, platformIcon, refreshSync, setSyncView, syncApi, useSyncView, type PairOutcome } from "../lib/sync";
import { setThemePref } from "../lib/theme";
import type { Settings, SettingsView, ThemePref } from "../lib/types";
import { ClearHistoryDialog } from "../components/ClearHistoryDialog";
import { Button, Segmented, Switch } from "../components/ui";
import { Row, Section, Select } from "../dashboard/SettingsDialog";

function ago(t: number | null | undefined): string {
  if (!t) return "never";
  if (Date.now() - t < 45_000) return "just now";
  return `${formatDistanceToNowStrict(t)} ago`;
}

const SIGNALS: Record<string, string> = {
  CLICK_COPY: "automatic (Copy tap)",
  TOAST_COPIED: "automatic (“Copied” message)",
  SYSUI_CLIPBOARD: "automatic (clipboard preview)",
  CLIPBOARD_UI: "automatic (clipboard panel)",
  APP_FOREGROUND: "opening ClipTrail",
  QS_TILE: "the Quick Settings tile",
  NOTIFICATION: "the notification button",
  SHARE: "Share",
  PROCESS_TEXT: "the text menu",
  IN_APP: "the Save button",
};

/** Settings as a full-screen sheet on the phone. */
export function MobileSettings({
  status,
  onStatus,
  pairLink,
  onPairLinkUsed,
  onClose,
}: {
  status: AndroidStatus | null;
  onStatus: (s: AndroidStatus) => void;
  pairLink: string | null;
  onPairLinkUsed: () => void;
  onClose: () => void;
}) {
  const [view, setView] = useState<SettingsView | null>(null);
  const [errors, setErrors] = useState<Record<string, string | null>>({});
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    const load = () => void api.getSettings().then(setView).catch(() => undefined);
    load();
    const a = listen("settings-changed", load);
    const b = listen("capture-changed", load);
    return () => {
      a();
      b();
    };
  }, []);

  const save = async (patch: Partial<Settings>, field: string) => {
    if (!view) return;
    try {
      setView(await api.updateSettings({ ...view.settings, ...patch }));
      setErrors((e) => ({ ...e, [field]: null }));
    } catch (e) {
      setErrors((x) => ({ ...x, [field]: errorMessage(e) }));
    }
  };

  const s = view?.settings;

  return (
    <div className="anim-sheet fixed inset-0 z-40 flex flex-col bg-bg">
      <div className="flex h-12 shrink-0 items-center gap-1 border-b border-line bg-panel px-1.5">
        <button
          aria-label="Back"
          onClick={onClose}
          className="grid h-10 w-10 place-items-center rounded-xl text-fg-2 active:bg-active"
        >
          <ArrowLeft size={21} strokeWidth={2} />
        </button>
        <span className="text-[15px] font-semibold text-fg">Settings</span>
      </div>

      <div className="scroll-y min-h-0 flex-1">
        <div className="space-y-6 px-4 pt-4 pb-10">
          <DevicesSection pairLink={pairLink} onPairLinkUsed={onPairLinkUsed} />

          {isAndroid && <CaptureSection status={status} onStatus={onStatus} view={view} />}

          {s && (
            <>
              <Section title="Appearance">
                <div className="px-4 py-3.5">
                  <div className="mb-2.5 text-[13px] font-medium text-fg">Theme</div>
                  <Segmented<ThemePref>
                    value={s.theme}
                    onChange={(t) => {
                      setThemePref(t, false);
                      void save({ theme: t }, "theme");
                    }}
                    options={[
                      { value: "system", label: "System", icon: Monitor },
                      { value: "light", label: "Light", icon: Sun },
                      { value: "dark", label: "Dark", icon: Moon },
                    ]}
                  />
                </div>
              </Section>

              <Section title="Privacy">
                <Row
                  title="Clean links"
                  desc="Remove tracking parameters (utm_*, fbclid, …) from links you copy."
                  error={errors.cleanLinks}
                >
                  <Switch
                    label="Clean tracking parameters from links"
                    checked={s.clean_links}
                    onChange={(v) => void save({ clean_links: v }, "cleanLinks")}
                  />
                </Row>
                <Row title="Sensitive clips" desc="One-time codes and keys are masked and deleted after:" error={errors.ttl}>
                  <Select
                    value={String(s.sensitive_ttl_minutes)}
                    onChange={(v) => void save({ sensitive_ttl_minutes: Number(v) }, "ttl")}
                    options={[
                      ["5", "5 min"],
                      ["15", "15 min"],
                      ["60", "1 hour"],
                      ["1440", "1 day"],
                      ["10080", "7 days"],
                    ]}
                  />
                </Row>
                <div className="flex items-start gap-2 px-4 py-3 text-[12px] leading-[1.45] text-fg-3">
                  <ShieldCheck size={14} className="mt-[1px] shrink-0 text-success" />
                  Copies an app marks as sensitive (passwords) and copies made in password managers are never saved.
                </div>
              </Section>

              <Section title="Storage">
                <Row title="Keep history" desc="Pinned clips are always kept." error={errors.retention}>
                  <Select
                    value={s.retention_days == null ? "forever" : String(s.retention_days)}
                    onChange={(v) => void save({ retention_days: v === "forever" ? null : Number(v) }, "retention")}
                    options={[
                      ["forever", "Forever"],
                      ["365", "1 year"],
                      ["90", "90 days"],
                      ["30", "30 days"],
                    ]}
                  />
                </Row>
              </Section>

              <Section title="Danger zone">
                <Row title="Clear history" desc="Permanently delete the clips saved on this phone.">
                  <Button size="sm" variant="danger" icon={Trash2} onClick={() => setClearing(true)}>
                    Clear history…
                  </Button>
                </Row>
              </Section>
            </>
          )}

          <div className="flex items-center gap-3.5 rounded-2xl border border-line bg-panel px-4 py-3.5">
            <img src="/logo.svg" alt="" className="h-9 w-9" />
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-semibold text-fg">ClipTrail {view?.version ?? ""}</div>
              <div className="text-[12px] leading-[1.45] text-fg-3">
                Your clips stay on this phone, and go only to computers you pair, end-to-end encrypted. No
                telemetry. MIT / Apache-2.0.
              </div>
            </div>
          </div>
        </div>
      </div>
      {clearing && <ClearHistoryDialog onClose={() => setClearing(false)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Capture: automatic-capture toggle + Samsung onboarding checklist

function CaptureSection({
  status: st,
  onStatus,
  view,
}: {
  status: AndroidStatus | null;
  onStatus: (s: AndroidStatus) => void;
  view: SettingsView | null;
}) {
  const [error, setError] = useState<string | null>(null);
  const run = (f: () => Promise<unknown>) => {
    f()
      .then(() => androidApi.status().then(onStatus))
      .then(() => setError(null))
      .catch((e) => setError(errorMessage(e)));
  };
  const open = (t: SettingsScreen) => run(() => androidApi.open(t));
  const capture = view?.capture;

  return (
    <Section title="Capture">
      <Row
        title="Automatic capture (beta)"
        desc="Saves what you copy in other apps, using an accessibility service that only notices Copy taps and “Copied” messages. It never reads the screen."
        error={error}
      >
        <Switch
          label="Automatic capture"
          checked={st?.autoCapture ?? false}
          disabled={!st}
          onChange={(v) => run(() => androidApi.setAutoCapture(v))}
        />
      </Row>

      {st?.autoCapture && (
        <div className="space-y-1 px-2 py-2">
          <Step
            n={1}
            icon={Hand}
            done={st.serviceEnabled && st.serviceConnected}
            title={
              st.serviceEnabled && !st.serviceConnected
                ? "Accessibility: on but not running — switch it off and on"
                : "Turn on ClipTrail in Accessibility"
            }
            detail="Accessibility › Installed apps › ClipTrail automatic capture › On › Allow."
            action={{ label: "Open", run: () => open("accessibility") }}
          />
          {st.restrictedSettings && !(st.serviceEnabled && st.serviceConnected) && (
            <Step
              n={2}
              icon={LockOpen}
              done={null}
              title="Switch greyed out? Allow restricted settings"
              detail="Android blocks this for apps installed from a file. Try step 1 once, then here tap ⋮ (top right) › Allow restricted settings, confirm, and do step 1 again."
              action={{ label: "App info", run: () => open("appInfo") }}
            />
          )}
          <Step
            n={st.restrictedSettings && !(st.serviceEnabled && st.serviceConnected) ? 3 : 2}
            icon={Bell}
            done={st.notificationsAllowed}
            title="Allow notifications"
            detail="Shows a quiet “ClipTrail is saving your copies” notification with a Save clipboard button."
            action={{ label: "Allow", run: () => run(() => androidApi.requestNotifications()) }}
          />
          <Step
            n={st.restrictedSettings && !(st.serviceEnabled && st.serviceConnected) ? 4 : 3}
            icon={BatteryCharging}
            done={st.batteryUnrestricted}
            title="Keep ClipTrail running"
            detail={
              st.samsung
                ? "Allow unrestricted battery use. Then in Battery › Background usage limits: add ClipTrail to Never sleeping apps, and turn off Put unused apps to sleep."
                : "Allow unrestricted battery use so Android doesn't stop capture in the background."
            }
            action={{ label: "Allow", run: () => open("batteryOptimization") }}
            extra={st.samsung ? { label: "Battery settings", run: () => open("battery") } : undefined}
          />
        </div>
      )}

      <Row
        title="Pause saving"
        desc={
          capture?.paused
            ? capture.until
              ? `Paused — resumes in ${untilLabel(capture.until)}.`
              : "Paused until you resume."
            : "Nothing is saved while paused, from any path."
        }
      >
        {capture?.paused ? (
          <Button size="sm" variant="primary" onClick={() => void api.resumeCapture()}>
            Resume
          </Button>
        ) : (
          <div className="flex gap-1.5">
            <Button size="sm" onClick={() => void api.pauseCapture(15)}>
              15 min
            </Button>
            <Button size="sm" onClick={() => void api.pauseCapture(60)}>
              1 hour
            </Button>
          </div>
        )}
      </Row>

      <div className="px-4 py-3.5">
        <div className="text-[13px] font-medium text-fg">Other ways to save</div>
        <ul className="mt-1.5 space-y-1 text-[12.5px] leading-[1.45] text-fg-3">
          <li>
            <span className="font-medium text-fg-2">Save clipboard</span> tile: swipe down twice › ✎ › drag it in.
          </li>
          <li>
            <span className="font-medium text-fg-2">Share › Save to ClipTrail</span> from any app (text and images).
          </li>
          <li>
            Select text › ⋮ › <span className="font-medium text-fg-2">Save to ClipTrail</span>.
          </li>
          <li>Opening ClipTrail saves what's on the clipboard.</li>
        </ul>
        {st && st.lastCaptureAt > 0 && (
          <div className="mt-2.5 text-[12px] text-fg-3">
            Last saved {ago(st.lastCaptureAt)}
            {st.lastSignal ? ` via ${SIGNALS[st.lastSignal] ?? st.lastSignal}` : ""} · {st.autoCaptures} automatic,{" "}
            {st.manualCaptures} manual
          </div>
        )}
      </div>
    </Section>
  );
}

function Step({
  n,
  icon: Icon,
  done,
  title,
  detail,
  action,
  extra,
}: {
  n: number;
  icon: typeof Hand;
  /** null = informational */
  done: boolean | null;
  title: string;
  detail: string;
  action: { label: string; run: () => void };
  extra?: { label: string; run: () => void };
}) {
  return (
    <div className={cx("flex gap-3 rounded-xl px-2.5 py-2.5", done === false && "bg-hover")}>
      <span
        className={cx(
          "mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-full text-[12px] font-semibold",
          done ? "bg-[color-mix(in_oklab,var(--success)_16%,transparent)] text-success" : "bg-accent-soft text-accent-text",
        )}
      >
        {done ? <CircleCheck size={17} strokeWidth={2.2} /> : <Icon size={16} strokeWidth={2} aria-label={`Step ${n}`} />}
      </span>
      <div className="min-w-0 flex-1">
        <div className={cx("text-[13.5px] font-medium", done ? "text-fg-3" : "text-fg")}>{title}</div>
        {!done && <div className="mt-0.5 text-[12px] leading-[1.45] text-fg-3">{detail}</div>}
        {!done && (
          <div className="mt-2 flex gap-1.5">
            <Button size="sm" variant={done === false ? "primary" : "secondary"} onClick={action.run}>
              {action.label}
            </Button>
            {extra && (
              <Button size="sm" onClick={extra.run}>
                {extra.label}
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Devices: pair with the computer (QR / link), status, unpair

function DevicesSection({ pairLink, onPairLinkUsed }: { pairLink: string | null; onPairLinkUsed: () => void }) {
  const v = useSyncView();
  const [link, setLink] = useState("");
  const [pasting, setPasting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [paired, setPaired] = useState<PairOutcome | null>(null);
  const [confirmUnpair, setConfirmUnpair] = useState(false);
  const started = useRef<string | null>(null);

  useEffect(() => refreshSync(), []);

  const pair = async (uri: string) => {
    setBusy(true);
    setError(null);
    try {
      const out = await syncApi.pair(uri.trim());
      setPaired(out);
      setPasting(false);
      setLink("");
      refreshSync();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  // Opened from a cliptrail://pair link: pair right away (the check code is shown after).
  useEffect(() => {
    if (!pairLink || started.current === pairLink || !v) return;
    started.current = pairLink;
    onPairLinkUsed();
    if (v.status.configured) {
      setError("This phone is already paired. Unpair it first to pair with another computer.");
      return;
    }
    setLink(pairLink);
    void pair(pairLink);
  }, [pairLink, v, onPairLinkUsed]);

  const scan = async () => {
    setError(null);
    try {
      const r = await androidApi.scanQr();
      if (r.text) await pair(r.text);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  if (!v) return null;
  const st = v.status;
  const isClient = st.configured && st.role === "client";
  const hub = otherDevices(v).find((d) => d.id !== st.device_id && !d.is_self);
  const HubIcon = platformIcon(hub?.platform ?? "windows");

  return (
    <Section title="Your computer">
      {paired && (
        <div className="anim-pop m-3 flex items-start gap-3 rounded-xl border border-line bg-panel-2 px-3.5 py-3">
          <CircleCheck size={19} className="mt-0.5 shrink-0 text-success" />
          <div className="min-w-0 flex-1 text-[13px]">
            <div className="font-semibold text-fg">Paired with {paired.hub_name}</div>
            <div className="mt-0.5 leading-[1.45] text-fg-3">
              Check code{" "}
              <span className="rounded-md bg-hover px-1.5 py-0.5 font-mono text-[14px] font-semibold tracking-[0.12em] text-fg">
                {paired.check_code}
              </span>{" "}
              should match the one on your computer. If it doesn't, unpair below.
            </div>
          </div>
        </div>
      )}

      {!isClient ? (
        <div className="px-4 py-4">
          <div className="flex items-start gap-3">
            <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent-soft text-accent-text">
              <Monitor size={19} strokeWidth={2} />
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-[14px] font-semibold text-fg">Sync with your computer</div>
              <div className="mt-0.5 text-[12.5px] leading-[1.45] text-fg-3">
                On the computer: ClipTrail › Settings › Devices › Pair a phone. Same Wi-Fi; end-to-end encrypted;
                optional — ClipTrail works fine on its own.
              </div>
            </div>
          </div>
          <div className="mt-3.5 grid grid-cols-2 gap-2">
            <Button variant="primary" icon={QrCode} disabled={busy} onClick={() => void scan()} className="h-10 text-[13px]">
              Scan QR code
            </Button>
            <Button icon={Link2} disabled={busy} onClick={() => setPasting((p) => !p)} className="h-10 text-[13px]">
              Paste link
            </Button>
          </div>
          {pasting && (
            <div className="anim-drop mt-3 space-y-2">
              <textarea
                value={link}
                onChange={(e) => setLink(e.target.value)}
                placeholder="cliptrail://pair?…"
                rows={3}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
                className="code block w-full resize-none rounded-xl border border-line bg-panel-2 px-3 py-2.5 text-[12px] break-all text-fg outline-none focus:border-accent"
              />
              <div className="flex gap-2">
                <Button
                  icon={ClipboardPaste}
                  onClick={() =>
                    void navigator.clipboard
                      ?.readText()
                      .then((t) => setLink(t.trim()))
                      .catch(() => setError("Couldn't read the clipboard; long-press the box and paste"))
                  }
                >
                  Paste
                </Button>
                <Button
                  variant="primary"
                  className="flex-1"
                  disabled={busy || !link.trim().startsWith("cliptrail://pair")}
                  onClick={() => void pair(link)}
                >
                  {busy ? "Pairing…" : "Pair"}
                </Button>
              </div>
            </div>
          )}
          {busy && !pasting && <div className="mt-3 text-[12.5px] text-fg-3">Pairing…</div>}
          {error && <div className="anim-fade mt-3 text-[12.5px] font-medium text-danger">{error}</div>}
        </div>
      ) : (
        <>
          <div className="flex items-center gap-3 px-4 py-3.5">
            <span className="relative grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-hover text-fg-2">
              <HubIcon size={18} strokeWidth={1.9} />
              {st.connected && (
                <span className="absolute -right-0.5 -bottom-0.5 h-3 w-3 rounded-full border-2 border-[var(--panel)] bg-success" />
              )}
            </span>
            <div className="min-w-0 flex-1">
              <div className="truncate text-[14px] font-semibold text-fg">{st.hub_name ?? "Your computer"}</div>
              <div className="truncate text-[12px] text-fg-3">
                {!v.enabled
                  ? "Sync is off"
                  : st.connected
                    ? "Connected · syncing"
                    : `Not connected${st.last_error ? ` · ${st.last_error}` : " · same Wi-Fi?"}`}
              </div>
            </div>
            <Switch
              label="Sync with my computer"
              checked={v.enabled}
              onChange={(on) =>
                void syncApi
                  .setEnabled(on)
                  .then(setSyncView)
                  .catch((e) => setError(errorMessage(e)))
              }
            />
          </div>
          {(st.outbox > 0 || st.pending_images > 0) && (
            <div className="px-4 py-2.5 text-[12px] text-fg-3">
              {st.outbox > 0 && `${st.outbox} change${st.outbox === 1 ? "" : "s"} waiting to send. `}
              {st.pending_images > 0 && `${st.pending_images} image${st.pending_images === 1 ? "" : "s"} to fetch.`}
            </div>
          )}
          <div className="flex items-center gap-3 px-4 py-3">
            <div className="min-w-0 flex-1 text-[12px] leading-[1.45] text-fg-3">
              Unpairing stops sync. Clips on this phone stay.
            </div>
            <Button
              size="sm"
              variant={confirmUnpair ? "danger" : "secondary"}
              icon={Unplug}
              onClick={() => {
                if (!confirmUnpair) {
                  setConfirmUnpair(true);
                  window.setTimeout(() => setConfirmUnpair(false), 3500);
                  return;
                }
                setConfirmUnpair(false);
                setPaired(null);
                void syncApi
                  .unpair()
                  .then((ok) => {
                    setError(ok ? null : "Unpaired here. The computer was unreachable; remove the phone there too.");
                    refreshSync();
                  })
                  .catch((e) => setError(errorMessage(e)));
              }}
            >
              {confirmUnpair ? "Tap to confirm" : "Unpair"}
            </Button>
          </div>
          {error && <div className="px-4 pb-3 text-[12.5px] font-medium text-danger">{error}</div>}
        </>
      )}
    </Section>
  );
}
