import { useEffect, useState } from "react";
import { EyeOff, ShieldCheck } from "lucide-react";
import { api, errorMessage, listen } from "../lib/api";
import { prettyKeys } from "../lib/shortcut";
import type { SettingsView } from "../lib/types";
import { Button, Kbd, Switch } from "../components/ui";

/** First-launch card (desktop): start-at-login (on by default for fresh
 *  installs), the tray icon, and the hotkeys. Shown once; any way of
 *  closing it counts as seen. */
export function Welcome() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    const load = () => void api.getSettings().then(setView).catch(() => undefined);
    load();
    return listen("settings-changed", load);
  }, []);

  const open = view != null && view.platform !== "android" && view.settings.welcome_seen === false;

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        void done();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  if (!open || !view) return null;
  const s = view.settings;
  const isWin = view.platform === "windows";
  const isMac = view.platform === "macos";
  const trayName = isMac ? "menu bar" : "tray";

  async function done() {
    if (closing || !view) return;
    setClosing(true);
    try {
      setView(await api.updateSettings({ ...view.settings, welcome_seen: true }));
    } catch (e) {
      setError(errorMessage(e));
      setClosing(false);
    }
  }

  const setAutostart = (on: boolean) =>
    void api
      .setAutostart(on)
      .then(() => {
        setView({ ...view, autostart: on });
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));

  const setTray = (on: boolean) =>
    void api
      .updateSettings({ ...s, show_tray_icon: on })
      .then((v) => {
        setView(v);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));

  return (
    <div className="fixed inset-0 z-50 grid place-items-center p-6">
      <div className="anim-fade absolute inset-0 bg-[rgb(10_10_14/0.32)] backdrop-blur-[2px] dark:bg-[rgb(0_0_0/0.5)]" />
      <div
        role="dialog"
        aria-modal
        aria-label="Welcome to ClipTrail"
        className="anim-pop relative w-full max-w-[460px] overflow-hidden rounded-2xl border border-line bg-elevated shadow-pop"
      >
        <div className="flex flex-col items-center bg-accent-soft px-6 pt-7 pb-5 text-center">
          <img src="/logo.svg" alt="" className="h-12 w-12" draggable={false} />
          <h2 className="mt-3 text-[18px] font-semibold tracking-[-0.02em] text-fg">Welcome to ClipTrail</h2>
          <p className="mt-1 max-w-[340px] text-[12.5px] leading-[1.5] text-fg-2">
            Everything you copy is saved from now on, searchable and private to your devices.
          </p>
        </div>

        <div className="space-y-4 px-6 py-5">
          <div className="space-y-2">
            <Shortcut label="Search and paste from anywhere" keys={s.picker_shortcut} />
            {s.dashboard_shortcut && <Shortcut label="Open this window" keys={s.dashboard_shortcut} />}
          </div>

          <div className="divide-y divide-[var(--border)] rounded-xl border border-line bg-panel">
            <Toggle
              title={isWin ? "Start with Windows" : "Start at login"}
              desc="Keeps your history complete. Starts quietly, no window."
              checked={view.autostart}
              onChange={setAutostart}
            />
            <Toggle
              title={`Show ${trayName} icon`}
              desc={
                s.show_tray_icon !== false
                  ? isWin
                    ? "Next to the clock, or under ^ in the taskbar."
                    : `Quick access from the ${trayName}.`
                  : "Open ClipTrail with the shortcuts above, or by launching it again."
              }
              checked={s.show_tray_icon !== false}
              onChange={setTray}
            />
          </div>

          <p className="flex items-start gap-2 text-[12px] leading-[1.45] text-fg-3">
            <EyeOff size={13} className="mt-[2px] shrink-0" />
            Closing this window doesn't stop ClipTrail. It keeps saving in the background.
          </p>
          {error && <div className="anim-fade text-[12px] font-medium text-danger">{error}</div>}
        </div>

        <div className="flex items-center gap-3 border-t border-line px-6 py-3.5">
          <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[11.5px] text-fg-3">
            <ShieldCheck size={12.5} className="shrink-0 text-success" />
            Local-first. No telemetry.
          </span>
          <Button variant="primary" disabled={closing} onClick={() => void done()}>
            Get started
          </Button>
        </div>
      </div>
    </div>
  );
}

function Shortcut({ label, keys }: { label: string; keys: string }) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-lg bg-panel-2 px-3.5 py-2.5">
      <span className="text-[12.5px] font-medium text-fg">{label}</span>
      <span className="flex shrink-0 items-center gap-1">
        {prettyKeys(keys).map((k) => (
          <Kbd key={k} className="h-[22px] min-w-[22px] px-1.5 text-[11.5px] text-fg-2">
            {k}
          </Kbd>
        ))}
      </span>
    </div>
  );
}

function Toggle({
  title,
  desc,
  checked,
  onChange,
}: {
  title: string;
  desc: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-5 px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-fg">{title}</div>
        <div className="mt-0.5 text-[12px] leading-[1.45] text-fg-3">{desc}</div>
      </div>
      <Switch label={title} checked={checked} onChange={onChange} />
    </div>
  );
}
