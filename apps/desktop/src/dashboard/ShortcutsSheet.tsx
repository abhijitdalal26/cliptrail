import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { MOD } from "../lib/hooks";
import { prettyKeys } from "../lib/shortcut";
import { Modal } from "../components/Modal";
import { Kbd } from "../components/ui";

type Entry = [string, string[][]];

export function ShortcutsSheet({ onClose }: { onClose: () => void }) {
  const [picker, setPicker] = useState("Ctrl+Shift+Space");
  const [dash, setDash] = useState<string | null>("Ctrl+Alt+H");
  useEffect(() => {
    void api
      .getSettings()
      .then((v) => {
        setPicker(v.settings.picker_shortcut);
        setDash(v.settings.dashboard_shortcut);
      })
      .catch(() => undefined);
  }, []);

  const k = (s: string) => s.split("+").map((x) => (x === "Ctrl" ? MOD : x));
  const global: Entry[] = [
    ["Open the quick picker", [prettyKeys(picker)]],
    ...(dash ? ([["Show or hide the dashboard", [prettyKeys(dash)]]] as Entry[]) : []),
  ];
  const dashboard: Entry[] = [
    ["Search", [["/"], k("Ctrl+F")]],
    ["Move selection", [["↑"], ["↓"]]],
    ["Copy selected clip", [["Enter"]]],
    ["Copy as plain text", [["Shift", "Enter"]]],
    ["Add to / remove from favorites", [k("Ctrl+P")]],
    ["Delete (with undo)", [["Del"]]],
    ["Undo delete", [k("Ctrl+Z")]],
    ["Clear search / filters", [["Esc"]]],
    ["Settings", [k("Ctrl+,")]],
    ["This sheet", [["?"]]],
  ];
  const picker_: Entry[] = [
    ["Paste into the previous app", [["Enter"]]],
    ["Paste as plain text", [["Shift", "Enter"]]],
    ["Actions: clean link, color as RGB, …", [k("Ctrl+Enter"), ["→"]]],
    ["In the action list: paste the result", [["↑"], ["↓"], ["Enter"]]],
    ["Close the action list", [["←"], ["Esc"]]],
    ["Quick-paste item 1–9", [k("Ctrl+1"), ["…"], k("Ctrl+9")]],
    ["Next / previous kind", [["Tab"], ["Shift", "Tab"]]],
    ["Move selection", [["↑"], ["↓"]]],
    ["Add to / remove from favorites", [k("Ctrl+P")]],
    ["Open the dashboard", [k("Ctrl+D")]],
    ["Close", [["Esc"]]],
  ];

  return (
    <Modal title="Keyboard shortcuts" subtitle="Everything works without a mouse" onClose={onClose} width={720}>
      <div className="grid grid-cols-2 gap-x-8 gap-y-6 px-6 py-5">
        <div className="space-y-6">
          <Group title="Anywhere" entries={global} />
          <Group title="Quick picker" entries={picker_} />
        </div>
        <Group title="Dashboard" entries={dashboard} />
      </div>
    </Modal>
  );
}

function Group({ title, entries }: { title: string; entries: Entry[] }) {
  return (
    <div>
      <h3 className="mb-1.5 text-[11px] font-semibold tracking-[0.05em] text-fg-3 uppercase">{title}</h3>
      <div className="divide-y divide-[var(--border)]">
        {entries.map(([label, combos]) => (
          <div key={label} className="flex items-center justify-between gap-4 py-2">
            <span className="text-[12.5px] text-fg-2">{label}</span>
            <span className="flex shrink-0 items-center gap-1">
              {combos.map((c, i) => (
                <span key={i} className="flex items-center gap-0.5">
                  {i > 0 && c[0] !== "…" && combos[i - 1][0] !== "…" && (
                    <span className="mx-0.5 text-[11px] text-fg-4">or</span>
                  )}
                  {c[0] === "…" ? (
                    <span className="text-[11px] text-fg-4">–</span>
                  ) : (
                    c.map((x) => (
                      <Kbd key={x} className="h-5 min-w-5 text-[11px] text-fg-2">
                        {x}
                      </Kbd>
                    ))
                  )}
                </span>
              ))}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
