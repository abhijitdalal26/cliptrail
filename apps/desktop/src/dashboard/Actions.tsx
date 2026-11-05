import { useEffect, useRef, useState } from "react";
import { Ellipsis } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { ACTION_META, PRIMARY_ACTIONS } from "../lib/actions";
import { cx } from "../lib/cx";
import { toast } from "../lib/toast";
import type { ClipSummary, Transform } from "../lib/types";

const MAX_SHOWN = 320;
const MAX_DIFF = 20_000;

/** Type-aware actions for a clip: pills that copy the text transformed
 *  (clean link, color as RGB, prettify JSON, ...). Hover or focus a pill to
 *  see exactly what it will copy. */
export function Actions({
  summary,
  original,
  bare,
}: {
  summary: ClipSummary;
  original: string | null;
  /** In the clip panel's Actions tab: no heading, and a note when there's nothing to offer. */
  bare?: boolean;
}) {
  const id = summary.id;
  const eligible = !summary.masked && !["sensitive", "image", "file"].includes(summary.kind);
  const [actions, setActions] = useState<Transform[]>([]);
  const [active, setActive] = useState<Transform | null>(null);
  const [result, setResult] = useState<{ t: Transform; text: string | null } | null>(null);
  const [menu, setMenu] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const cache = useRef(new Map<Transform, Promise<string | null>>());

  useEffect(() => {
    if (!eligible) return;
    let alive = true;
    api
      .clipActions(id)
      .then((a) => alive && setActions(a))
      .catch(() => alive && setActions([]));
    return () => {
      alive = false;
    };
  }, [id, eligible]);

  useEffect(() => {
    if (!menu) return;
    const down = (e: MouseEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setMenu(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setMenu(false);
        setActive(null);
      }
    };
    document.addEventListener("mousedown", down);
    window.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("mousedown", down);
      window.removeEventListener("keydown", key, true);
    };
  }, [menu]);

  if (actions.length === 0) {
    return bare ? <div className="px-5 py-2 text-[12.5px] text-fg-3">No quick actions for this clip.</div> : null;
  }

  const show = (t: Transform) => {
    setActive(t);
    let p = cache.current.get(t);
    if (!p) {
      p = api.transformClip(id, t).catch(() => null);
      cache.current.set(t, p);
    }
    void p.then((text) => setResult({ t, text }));
  };
  const hide = () => setActive(null);

  const run = (t: Transform) => {
    setMenu(false);
    setActive(null);
    api
      .copyClip(id, false, t)
      .then(() => toast(ACTION_META[t].done))
      .catch((e) => toast({ message: errorMessage(e), tone: "error" }));
  };

  const inlineCount = Math.min(actions.length, PRIMARY_ACTIONS);
  const inline = actions.slice(0, inlineCount);
  const more = actions.slice(inlineCount);
  const previewing = active && result?.t === active ? result.text : null;

  return (
    <div ref={wrap} className="relative -mt-1 px-5 pb-4">
      {!bare && <div className="mb-1.5 text-[11px] font-semibold tracking-[0.05em] text-fg-3 uppercase">Actions</div>}
      <div className="flex flex-wrap gap-1.5">
        {inline.map((t) => (
          <ActionPill key={t} t={t} on={active === t} onRun={run} onShow={show} onHide={hide} />
        ))}
        {more.length > 0 && (
          <button
            aria-haspopup="menu"
            aria-expanded={menu}
            onClick={() => setMenu((v) => !v)}
            className={cx(
              "inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-[12px] font-medium transition-colors duration-150",
              menu
                ? "border-transparent bg-accent-soft text-accent-text"
                : "border-line text-fg-2 hover:border-line-strong hover:bg-hover hover:text-fg",
            )}
          >
            <Ellipsis size={13} strokeWidth={2} />
            More
          </button>
        )}
      </div>

      {menu && (
        <div
          role="menu"
          className="anim-pop absolute top-full right-5 left-5 z-40 mt-1 overflow-hidden rounded-xl border border-line bg-elevated p-1 shadow-pop"
        >
          {more.map((t) => {
            const m = ACTION_META[t];
            const Icon = m.icon;
            return (
              <button
                key={t}
                role="menuitem"
                onClick={() => run(t)}
                onMouseEnter={() => show(t)}
                onMouseLeave={hide}
                onFocus={() => show(t)}
                onBlur={hide}
                className="flex h-8 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[12.5px] font-medium text-fg-2 transition-colors hover:bg-hover hover:text-fg"
              >
                <Icon size={14} strokeWidth={2} className="shrink-0 text-fg-3" />
                {m.label}
              </button>
            );
          })}
          {active && more.includes(active) && (
            <div className="mt-1 border-t border-line px-1 pt-1.5 pb-1">
              <ResultBox original={original} text={previewing} />
            </div>
          )}
        </div>
      )}

      {!menu && active && inline.includes(active) && (
        <div className="anim-fade pointer-events-none absolute top-full right-5 left-5 z-30 mt-1 rounded-xl border border-line bg-elevated p-1.5 shadow-pop">
          <ResultBox original={original} text={previewing} />
        </div>
      )}
    </div>
  );
}

function ActionPill({
  t,
  on,
  onRun,
  onShow,
  onHide,
}: {
  t: Transform;
  on: boolean;
  onRun: (t: Transform) => void;
  onShow: (t: Transform) => void;
  onHide: () => void;
}) {
  const m = ACTION_META[t];
  const Icon = m.icon;
  return (
    <button
      onClick={() => onRun(t)}
      onMouseEnter={() => onShow(t)}
      onMouseLeave={onHide}
      onFocus={() => onShow(t)}
      onBlur={onHide}
      className={cx(
        "inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-[12px] font-medium whitespace-nowrap transition-colors duration-150 active:scale-[0.98]",
        on
          ? "border-transparent bg-accent-soft text-accent-text"
          : "border-line text-fg-2 hover:border-line-strong hover:bg-hover hover:text-fg",
      )}
    >
      <Icon size={13} strokeWidth={2} className="shrink-0" />
      {m.label}
    </button>
  );
}

/** The transformed text, with the part that changed highlighted. */
export function ResultBox({ original, text, compact }: { original: string | null; text: string | null; compact?: boolean }) {
  if (text === null) {
    return (
      <div className="space-y-1.5 px-2 py-1.5">
        <div className="skeleton h-3 w-[88%] rounded" />
        <div className="skeleton h-3 w-[54%] rounded" />
      </div>
    );
  }
  const { start, pre, mid, post, more, diffed } = segments(original, text);
  return (
    <div
      className={cx(
        "code overflow-hidden px-2 py-1.5 text-[11.5px] leading-[1.55] break-all whitespace-pre-wrap text-fg-2",
        compact ? "max-h-[64px]" : "max-h-[132px]",
      )}
    >
      {start > 0 && <span className="text-fg-4">…</span>}
      {pre}
      {mid ? (
        <mark className="rounded-[3px] bg-accent-soft px-px text-fg">{mid}</mark>
      ) : (
        diffed && <span aria-hidden className="mx-px inline-block h-[1.1em] w-[2px] translate-y-[2px] rounded bg-accent" />
      )}
      {post}
      {more && <span className="text-fg-4">…</span>}
    </div>
  );
}

/** Common prefix/suffix diff, windowed around the change. */
function segments(original: string | null, text: string) {
  const b = [...text];
  let s = 0;
  let e = b.length;
  let diffed = false;
  if (original !== null && original !== text && original.length < MAX_DIFF && text.length < MAX_DIFF) {
    diffed = true;
    const a = [...original];
    while (s < a.length && s < b.length && a[s] === b[s]) s++;
    let ea = a.length;
    while (ea > s && e > s && a[ea - 1] === b[e - 1]) {
      ea--;
      e--;
    }
  } else {
    s = e = 0; // nothing to highlight
  }
  const start = s > 80 ? s - 60 : 0;
  const end = Math.min(b.length, start + MAX_SHOWN);
  const midEnd = Math.min(e, end);
  const midStart = Math.min(s, end);
  return {
    start,
    pre: b.slice(start, midStart).join(""),
    mid: b.slice(midStart, midEnd).join(""),
    post: b.slice(midEnd, end).join(""),
    more: end < b.length,
    diffed,
  };
}
