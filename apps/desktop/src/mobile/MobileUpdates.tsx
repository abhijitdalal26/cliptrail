import { useEffect, useState, useSyncExternalStore } from "react";
import { Download, RefreshCw } from "lucide-react";
import { api, errorMessage, listen } from "../lib/api";
import { androidApi, type ApkUpdate } from "../lib/android";
import type { SettingsView } from "../lib/types";
import { Button, Switch } from "../components/ui";
import { Row, Section } from "../dashboard/SettingsDialog";

// Phone updates: a sideloaded app can't update itself, so ClipTrail only
// finds out about a newer release and offers the APK download; Android then
// installs it over this one. Checks are opt-in, like on the desktop.

const LAST_CHECK_KEY = "cliptrail-apk-update-check";
const CHECK_EVERY_MS = 24 * 60 * 60 * 1000;

interface State {
  result: ApkUpdate | null;
  checking: boolean;
  error: string | null;
}

let state: State = { result: null, checking: false, error: null };
const listeners = new Set<() => void>();
function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}
function subscribe(cb: () => void) {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

async function checkForApkUpdate() {
  if (state.checking) return;
  set({ checking: true, error: null });
  try {
    set({ result: await androidApi.updateCheck() });
    try {
      localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
    } catch {
      /* no storage: check again next launch */
    }
  } catch (e) {
    set({ error: errorMessage(e) });
  } finally {
    set({ checking: false });
  }
}

function lastCheck(): number {
  try {
    return Number(localStorage.getItem(LAST_CHECK_KEY)) || 0;
  } catch {
    return 0;
  }
}

function useSettingsView(): [SettingsView | null, (on: boolean) => Promise<void>] {
  const [view, setView] = useState<SettingsView | null>(null);
  useEffect(() => {
    const load = () => void api.getSettings().then(setView).catch(() => undefined);
    load();
    return listen("settings-changed", load);
  }, []);
  const choose = async (on: boolean) => {
    if (!view) return;
    setView(await api.updateSettings({ ...view.settings, auto_update_check: on }));
    if (on) void checkForApkUpdate();
  };
  return [view, choose];
}

function useApkUpdate() {
  return useSyncExternalStore(subscribe, () => state);
}

function download(u: ApkUpdate) {
  void api.openUrl(u.apkUrl).catch(() => undefined);
}

/** Home screen card: the one-time opt-in, then "a new version is available". */
export function UpdateCard() {
  const [view, choose] = useSettingsView();
  const u = useApkUpdate();
  const [dismissed, setDismissed] = useState<string | null>(null);
  const auto = view?.settings.auto_update_check;

  // Background check: at most once a day, only when the user opted in.
  useEffect(() => {
    if (auto === true && Date.now() - lastCheck() > CHECK_EVERY_MS) void checkForApkUpdate();
  }, [auto]);

  if (!view) return null;
  const found = u.result?.available ? u.result : null;
  if (found && dismissed !== found.version) {
    return (
      <div className="flex items-center gap-3 rounded-2xl border border-line bg-accent-soft px-3.5 py-3">
        <Download size={19} className="shrink-0 text-accent-text" />
        <div className="min-w-0 flex-1">
          <div className="text-[14px] font-semibold text-fg">ClipTrail {found.version} is available</div>
          <div className="text-[12.5px] text-fg-3">You have {found.currentVersion}. Your history is kept.</div>
          <div className="mt-2 flex gap-1.5">
            <Button size="sm" variant="primary" onClick={() => download(found)}>
              Download
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setDismissed(found.version)}>
              Later
            </Button>
          </div>
        </div>
      </div>
    );
  }
  if (auto == null) {
    return (
      <div className="rounded-2xl border border-line bg-panel px-3.5 py-3 shadow-soft">
        <div className="text-[14px] font-semibold text-fg">Check for updates automatically?</div>
        <div className="mt-0.5 text-[12.5px] leading-[1.4] text-fg-3">
          ClipTrail would ask GitHub once a day if there's a new version. Nothing about you or your clips is sent.
        </div>
        <div className="mt-2 flex gap-1.5">
          <Button size="sm" variant="primary" onClick={() => void choose(true).catch(() => undefined)}>
            Turn on
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void choose(false).catch(() => undefined)}>
            Not now
          </Button>
        </div>
      </div>
    );
  }
  return null;
}

/** Settings → Updates: the automatic-check switch and "Check now". */
export function UpdatesSection() {
  const [view, choose] = useSettingsView();
  const u = useApkUpdate();
  const [error, setError] = useState<string | null>(null);
  const r = u.result;
  const status = u.checking
    ? "Checking…"
    : u.error
      ? u.error
      : r
        ? r.available
          ? `Version ${r.version} is available (you have ${r.currentVersion}).`
          : `You have the latest version (${r.currentVersion}).`
        : "Asks GitHub once a day. Nothing about you or your clips is sent.";
  return (
    <Section title="Updates">
      <Row title="Check for updates automatically" desc={status} error={error}>
        <Switch
          label="Check for updates automatically"
          checked={view?.settings.auto_update_check === true}
          disabled={!view}
          onChange={(v) => void choose(v).then(() => setError(null), (e) => setError(errorMessage(e)))}
        />
      </Row>
      <div className="flex gap-1.5 px-4 pb-3.5">
        <Button size="sm" icon={RefreshCw} disabled={u.checking} onClick={() => void checkForApkUpdate()}>
          Check now
        </Button>
        {r?.available && (
          <Button size="sm" variant="primary" icon={Download} onClick={() => download(r)}>
            Download {r.version}
          </Button>
        )}
      </div>
    </Section>
  );
}
