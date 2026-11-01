import { useEffect, useState } from "react";
import { Download, ShieldCheck } from "lucide-react";
import { api, listen } from "../lib/api";
import { cx } from "../lib/cx";
import { installUpdate, releaseNotesUrl, useUpdates } from "../lib/updates";
import type { SettingsView } from "../lib/types";
import { Button } from "../components/ui";

function pct(downloaded: number, total: number | null): number | null {
  return total ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
}

/** Non-blocking cards at the top of the dashboard: the one-time opt-in for
 *  automatic checks, and "Version X is available". Never installs by itself. */
export function UpdateNotices() {
  const u = useUpdates();
  const [view, setView] = useState<SettingsView | null>(null);
  const [later, setLater] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const load = () => void api.getSettings().then(setView).catch(() => undefined);
    load();
    return listen("settings-changed", load);
  }, []);

  if (!view || view.platform === "android") return null;

  const choose = async (on: boolean) => {
    setSaving(true);
    try {
      setView(await api.updateSettings({ ...view.settings, auto_update_check: on }));
    } catch {
      /* stays visible; try again */
    } finally {
      setSaving(false);
    }
  };

  if (u.pending && later !== u.pending.version) {
    const p = u.pending;
    const progress = u.progress;
    const percent = progress?.phase === "downloading" ? pct(progress.downloaded, progress.total) : null;
    return (
      <div className="anim-drop mx-6 mb-2 rounded-xl border border-line bg-accent-soft px-3.5 py-2.5">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <Download size={16} className="shrink-0 text-accent-text" />
          <div className="min-w-0 flex-1 text-[12.5px]">
            <span className="font-semibold text-fg">Version {p.version} is available</span>
            <span className="text-fg-3"> · you have {p.currentVersion}</span>
          </div>
          {u.installing ? (
            <span className="text-[12px] font-medium text-fg-2">
              {progress?.phase === "installing"
                ? "Installing… ClipTrail will restart"
                : percent != null
                  ? `Downloading… ${percent}%`
                  : "Downloading…"}
            </span>
          ) : (
            <div className="flex items-center gap-1.5">
              <Button size="sm" variant="ghost" onClick={() => void api.openUrl(releaseNotesUrl(p.version)).catch(() => undefined)}>
                Release notes
              </Button>
              <Button size="sm" variant="primary" onClick={() => void installUpdate()}>
                Install &amp; restart
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setLater(p.version)}>
                Later
              </Button>
            </div>
          )}
        </div>
        {u.installing && (
          <div className="mt-2 h-1 overflow-hidden rounded-full bg-hover">
            <div
              className={cx("h-full rounded-full bg-accent transition-[width] duration-200", percent == null && "w-1/3 animate-pulse")}
              style={percent != null ? { width: `${percent}%` } : undefined}
            />
          </div>
        )}
        {u.error && <div className="mt-2 text-[12px] font-medium text-danger">{u.error}</div>}
      </div>
    );
  }

  if (view.settings.auto_update_check == null) {
    return (
      <div className="anim-drop mx-6 mb-2 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-xl border border-line bg-panel-2 px-3.5 py-2.5">
        <ShieldCheck size={16} className="shrink-0 text-success" />
        <div className="min-w-0 flex-1 text-[12.5px] leading-[1.45]">
          <span className="font-semibold text-fg">Check for updates automatically?</span>{" "}
          <span className="text-fg-3">
            ClipTrail will contact GitHub (no data about you or your clips is sent).
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          <Button size="sm" variant="primary" disabled={saving} onClick={() => void choose(true)}>
            Turn on
          </Button>
          <Button size="sm" variant="ghost" disabled={saving} onClick={() => void choose(false)}>
            Not now
          </Button>
        </div>
      </div>
    );
  }
  return null;
}
