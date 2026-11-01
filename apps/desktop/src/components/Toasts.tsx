import { CircleAlert, CircleCheck } from "lucide-react";
import { cx } from "../lib/cx";
import { dismiss, useToasts } from "../lib/toast";

export function Toasts({ compact }: { compact?: boolean }) {
  const toasts = useToasts();
  return (
    <div
      aria-live="polite"
      className={cx(
        "pointer-events-none fixed inset-x-0 z-[60] flex flex-col items-center gap-2",
        compact ? "bottom-12" : "bottom-5",
      )}
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className={cx(
            "anim-toast pointer-events-auto flex items-center gap-2.5 rounded-xl py-2 pr-2 pl-3 text-[12.5px] font-medium shadow-pop",
            "bg-[#1b1b20] text-white ring-1 ring-white/10 dark:bg-[#26262d]",
            !t.action && "pr-3.5",
          )}
        >
          {t.tone === "error" ? (
            <CircleAlert size={15} className="shrink-0 text-[#f87171]" />
          ) : (
            <CircleCheck size={15} className="shrink-0 text-[#4ade80]" />
          )}
          <span className="max-w-[420px] truncate">{t.message}</span>
          {t.action && (
            <button
              onClick={() => {
                t.action!.run();
                dismiss(t.id);
              }}
              className="ml-1 rounded-lg bg-white/10 px-2.5 py-1 text-[12px] font-semibold text-white transition-colors hover:bg-white/20"
            >
              {t.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
