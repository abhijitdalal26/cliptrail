import { useEffect, useState, type ReactNode } from "react";
import { ChevronDown, FolderOpen, Monitor, Moon, Plus, RotateCcw, ShieldCheck, Sun, X } from "lucide-react";
import { api, errorMessage, listen } from "../lib/api";
import { cx } from "../lib/cx";
import { useEvent } from "../lib/hooks";
import { untilLabel } from "../lib/format";
import { accelFromEvent, prettyKeys, validAccel } from "../lib/shortcut";
import { setThemePref } from "../lib/theme";
import type { Settings, SettingsView, ThemePref } from "../lib/types";
import { Modal } from "../components/Modal";
import { Button, Kbd, Segmented, Switch } from "../components/ui";

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const [view, setView] = useState<SettingsView | null>(null);
  const [errors, setErrors] = useState<Record<string, string | null>>({});

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

  if (!view) return null;
  const s = view.settings;
  const isWin = view.platform === "windows";

  const save = async (patch: Partial<Settings>, field: string) => {
    try {
      const v = await api.updateSettings({ ...s, ...patch });
      setView(v);
      setErrors((e) => ({ ...e, [field]: null }));
      return true;
    } catch (e) {
      setErrors((x) => ({ ...x, [field]: errorMessage(e) }));
      return false;
    }
  };

  return (
    <Modal title="Settings" subtitle="Saved automatically" onClose={onClose} width={680}>
      <div className="space-y-7 px-6 py-5">
        <Section title="General">
          <Row
            title={isWin ? "Start with Windows" : "Start at login"}
            desc="Launch quietly into the tray when you sign in."
          >
            <Switch
              label="Start at login"
              checked={view.autostart}
              onChange={(v) =>
                void api
                  .setAutostart(v)
                  .then(() => setView({ ...view, autostart: v }))
                  .catch((e) => setErrors((x) => ({ ...x, autostart: errorMessage(e) })))
              }
            />
          </Row>
          <Row
            title="Clipboard capture"
            desc={
              view.capture.paused
                ? view.capture.until
                  ? `Paused — resumes in ${untilLabel(view.capture.until)}.`
                  : "Paused until you resume it."
                : "Everything you copy is saved, except from excluded apps."
            }
            error={errors.autostart}
          >
            {view.capture.paused ? (
              <Button size="sm" variant="primary" onClick={() => void api.resumeCapture()}>
                Resume
              </Button>
            ) : (
              <div className="flex gap-1.5">
                <Button size="sm" onClick={() => void api.pauseCapture(15)}>
                  Pause 15 min
                </Button>
                <Button size="sm" onClick={() => void api.pauseCapture(60)}>
                  1 hour
                </Button>
                <Button size="sm" onClick={() => void api.pauseCapture(null)}>
                  Until resumed
                </Button>
              </div>
            )}
          </Row>
        </Section>

        <Section title="Shortcuts">
          <Row title="Quick picker" desc="Search and paste from anywhere." error={errors.picker}>
            <HotkeyInput
              value={s.picker_shortcut}
              onChange={(v) => save({ picker_shortcut: v }, "picker")}
              onError={(m) => setErrors((x) => ({ ...x, picker: m }))}
            />
          </Row>
          <Row title="Open dashboard" desc="Bring this window to the front." error={errors.dashboard}>
            <div className="flex items-center gap-2.5">
              {s.dashboard_shortcut && (
                <HotkeyInput
                  value={s.dashboard_shortcut}
                  onChange={(v) => save({ dashboard_shortcut: v }, "dashboard")}
                  onError={(m) => setErrors((x) => ({ ...x, dashboard: m }))}
                />
              )}
              <Switch
                label="Dashboard shortcut"
                checked={s.dashboard_shortcut != null}
                onChange={(on) => void save({ dashboard_shortcut: on ? "Ctrl+Alt+H" : null }, "dashboard")}
              />
            </div>
          </Row>
        </Section>

        <Section title="Appearance">
          <Row title="Theme" desc="Follow the system, or pick one.">
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
          </Row>
        </Section>

        <Section title="Privacy">
          <Row
            title="Sensitive clips"
            desc="One-time codes, API keys and card numbers are masked and deleted after:"
            error={errors.ttl}
          >
            <Select
              value={String(s.sensitive_ttl_minutes)}
              onChange={(v) => void save({ sensitive_ttl_minutes: Number(v) }, "ttl")}
              options={[
                ["5", "5 minutes"],
                ["15", "15 minutes"],
                ["60", "1 hour"],
                ["1440", "1 day"],
                ["10080", "7 days"],
              ]}
            />
          </Row>
          <ExcludedApps
            apps={s.excluded_apps}
            defaults={view.default_excluded_apps}
            isWin={isWin}
            error={errors.excluded}
            onChange={(apps) => void save({ excluded_apps: apps }, "excluded")}
          />
        </Section>

        <Section title="Storage">
          <Row title="Keep history" desc="Older clips are deleted automatically. Pinned clips are always kept." error={errors.retention}>
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
          <Row title="Data folder" desc={<span className="code text-[11.5px] break-all">{view.data_dir}</span>}>
            <Button size="sm" icon={FolderOpen} onClick={() => void api.openDataDir().catch(() => undefined)}>
              Open folder
            </Button>
          </Row>
        </Section>

        <div className="flex items-center gap-3.5 rounded-xl border border-line bg-panel-2 px-4 py-3.5">
          <img src="/logo.svg" alt="" className="h-9 w-9" />
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-semibold text-fg">ClipTrail {view.version}</div>
            <div className="flex items-center gap-1.5 text-[12px] text-fg-3">
              <ShieldCheck size={12.5} className="text-success" />
              Local-first. Your clips never leave this device. No telemetry.
            </div>
          </div>
          <span className="text-[11.5px] text-fg-3">MIT / Apache-2.0</span>
        </div>
      </div>
    </Modal>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h3 className="mb-2 px-1 text-[11px] font-semibold tracking-[0.05em] text-fg-3 uppercase">{title}</h3>
      <div className="divide-y divide-[var(--border)] overflow-visible rounded-xl border border-line bg-panel">
        {children}
      </div>
    </section>
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
      {error && <div className="anim-fade mt-2 text-[12px] font-medium text-danger">{error}</div>}
    </div>
  );
}

function Select({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: [string, string][];
}) {
  return (
    <span className="relative inline-flex">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className={cx(
          "h-8 appearance-none rounded-lg border border-line bg-panel py-0 pr-8 pl-3 text-[12.5px] font-medium text-fg shadow-soft",
          "outline-none hover:border-line-strong focus-visible:border-accent",
        )}
      >
        {options.map(([v, l]) => (
          <option key={v} value={v}>
            {l}
          </option>
        ))}
      </select>
      <ChevronDown
        size={14}
        strokeWidth={2.2}
        className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-fg-3"
      />
    </span>
  );
}

function HotkeyInput({
  value,
  onChange,
  onError,
}: {
  value: string;
  onChange: (v: string) => Promise<boolean>;
  onError: (m: string | null) => void;
}) {
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<string | null>(null);
  const change = useEvent(onChange);
  const fail = useEvent(onError);

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        setRecording(false);
        return;
      }
      const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win"].filter(Boolean);
      setHeld(mods.length ? mods.join("+") : null);
      const accel = accelFromEvent(e);
      if (!accel) return;
      const bad = validAccel(accel);
      if (bad) {
        fail(bad);
        return;
      }
      setRecording(false);
      fail(null);
      void change(accel);
    };
    const onBlur = () => setRecording(false);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [recording, change, fail]);

  return (
    <button
      onClick={() => {
        setHeld(null);
        setRecording((r) => !r);
      }}
      className={cx(
        "flex h-8 min-w-[150px] items-center justify-center gap-1 rounded-lg border px-2.5 transition-[border-color,box-shadow] duration-150",
        recording
          ? "border-accent shadow-[0_0_0_3px_var(--accent-soft)]"
          : "border-line bg-panel shadow-soft hover:border-line-strong",
      )}
    >
      {recording ? (
        <span className="text-[12px] font-medium text-accent-text">{held ? `${held}+…` : "Press keys…"}</span>
      ) : (
        prettyKeys(value).map((k) => (
          <Kbd key={k} className="h-5 min-w-5 text-[11px] text-fg-2">
            {k}
          </Kbd>
        ))
      )}
    </button>
  );
}

function ExcludedApps({
  apps,
  defaults,
  isWin,
  error,
  onChange,
}: {
  apps: string[];
  defaults: string[];
  isWin: boolean;
  error?: string | null;
  onChange: (apps: string[]) => void;
}) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const v = draft.trim().toLowerCase();
    if (!v) return;
    if (!apps.includes(v)) onChange([...apps, v]);
    setDraft("");
  };
  const missingDefaults = defaults.filter((d) => !apps.includes(d));
  return (
    <div className="px-4 py-3.5">
      <div className="flex items-start gap-6">
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-fg">Excluded apps</div>
          <div className="mt-0.5 text-[12px] leading-[1.45] text-fg-3">
            Copies from these apps are never saved. Password managers are excluded by default.
          </div>
        </div>
        {missingDefaults.length > 0 && (
          <Button size="sm" variant="ghost" icon={RotateCcw} onClick={() => onChange([...apps, ...missingDefaults])}>
            Restore defaults
          </Button>
        )}
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        {apps.map((a) => (
          <span
            key={a}
            className="code inline-flex h-7 items-center gap-1 rounded-lg border border-line bg-panel-2 pr-1 pl-2.5 text-[11.5px] text-fg-2"
          >
            {a}
            {defaults.includes(a) && <span className="font-sans text-[10.5px] text-fg-4">default</span>}
            <button
              aria-label={`Stop excluding ${a}`}
              onClick={() => onChange(apps.filter((x) => x !== a))}
              className="grid h-5 w-5 place-items-center rounded-md text-fg-3 hover:bg-hover hover:text-fg"
            >
              <X size={12} strokeWidth={2.2} />
            </button>
          </span>
        ))}
        <span className="inline-flex h-7 items-center gap-1 rounded-lg border border-dashed border-line-strong pr-1 pl-2.5">
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && add()}
            placeholder={isWin ? "app.exe" : "process name"}
            spellCheck={false}
            className="code w-[110px] bg-transparent text-[11.5px] text-fg outline-none"
          />
          <button
            aria-label="Add excluded app"
            onClick={add}
            className="grid h-5 w-5 place-items-center rounded-md text-fg-3 hover:bg-hover hover:text-fg"
          >
            <Plus size={12} strokeWidth={2.2} />
          </button>
        </span>
      </div>
      {error && <div className="mt-2 text-[12px] font-medium text-danger">{error}</div>}
    </div>
  );
}
