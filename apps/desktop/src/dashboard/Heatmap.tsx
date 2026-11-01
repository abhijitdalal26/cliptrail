import { useMemo } from "react";
import { addDays, format, startOfDay } from "date-fns";
import { cx } from "../lib/cx";
import { dayKey } from "../lib/format";
import { HEATMAP_WEEKS, heatmapStart } from "../lib/heat";

interface Props {
  counts: Map<string, number>;
  selected: string | null;
  onPick: (dayStartMs: number) => void;
}

export function Heatmap({ counts, selected, onPick }: Props) {
  const { weeks, months, max } = useMemo(() => {
    const start = heatmapStart();
    const today = startOfDay(Date.now()).getTime();
    const weeks: { date: Date; key: string; future: boolean }[][] = [];
    const months: { col: number; label: string }[] = [];
    let max = 0;
    for (let w = 0; w < HEATMAP_WEEKS; w++) {
      const col: { date: Date; key: string; future: boolean }[] = [];
      for (let d = 0; d < 7; d++) {
        const date = addDays(start, w * 7 + d);
        const key = dayKey(date.getTime());
        max = Math.max(max, counts.get(key) ?? 0);
        col.push({ date, key, future: date.getTime() > today });
      }
      const first = col[0].date;
      const prev = weeks[w - 1]?.[0].date;
      if (!prev || prev.getMonth() !== first.getMonth()) {
        if (w < HEATMAP_WEEKS - 1) months.push({ col: w, label: format(first, "MMM") });
      }
      weeks.push(col);
    }
    return { weeks, months, max };
  }, [counts]);

  const level = (n: number) => {
    if (n <= 0 || max === 0) return 0;
    const r = n / max;
    return r > 0.75 ? 4 : r > 0.45 ? 3 : r > 0.2 ? 2 : 1;
  };

  const CELL = 11;
  const GAP = 3;

  return (
    <div className="select-none">
      <div className="relative mb-1 h-3.5 text-[10.5px] text-fg-3" style={{ marginLeft: 22 }}>
        {months.map((m) => (
          <span key={m.col} className="absolute" style={{ left: m.col * (CELL + GAP) }}>
            {m.label}
          </span>
        ))}
      </div>
      <div className="flex">
        <div className="mr-1.5 flex w-4 flex-col text-[10px] leading-none text-fg-4" style={{ gap: GAP }}>
          {["", "M", "", "W", "", "F", ""].map((d, i) => (
            <span key={i} style={{ height: CELL }} className="flex items-center">
              {d}
            </span>
          ))}
        </div>
        <div className="flex" style={{ gap: GAP }}>
          {weeks.map((col, w) => (
            <div key={w} className="flex flex-col" style={{ gap: GAP }}>
              {col.map(({ date, key, future }) => {
                const n = counts.get(key) ?? 0;
                return (
                  <button
                    key={key}
                    disabled={future}
                    title={`${format(date, "EEE, MMM d")} · ${n === 0 ? "No clips" : n === 1 ? "1 clip" : `${n} clips`}`}
                    onClick={() => onPick(date.getTime())}
                    className={cx(
                      "rounded-[3px] transition-[transform,box-shadow] duration-150",
                      future ? "opacity-0" : "hover:scale-[1.25] hover:shadow-[0_0_0_1.5px_var(--fg-3)]",
                      selected === key && "shadow-[0_0_0_1.5px_var(--accent)]",
                    )}
                    style={{ width: CELL, height: CELL, background: `var(--heat-${level(n)})` }}
                  />
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export function HeatLegend() {
  return (
    <div className="flex items-center gap-1 text-[10.5px] text-fg-3">
      Less
      {[0, 1, 2, 3, 4].map((l) => (
        <span key={l} className="h-[9px] w-[9px] rounded-[2px]" style={{ background: `var(--heat-${l})` }} />
      ))}
      More
    </div>
  );
}
