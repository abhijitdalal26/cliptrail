import { useEffect, type ReactNode } from "react";
import { X } from "lucide-react";
import { cx } from "../lib/cx";
import { IconButton } from "./ui";

export function Modal({
  title,
  subtitle,
  onClose,
  children,
  width = 640,
  footer,
}: {
  title: string;
  subtitle?: string;
  onClose: () => void;
  children: ReactNode;
  width?: number;
  footer?: ReactNode;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // A control that records keys (the hotkey input) uses Esc to cancel.
      const t = e.target as HTMLElement | null;
      if (e.key === "Escape" && !t?.closest?.("[data-records-keys]")) {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 grid place-items-center p-6">
      <div
        className="anim-fade absolute inset-0 bg-[rgb(10_10_14/0.32)] backdrop-blur-[2px] dark:bg-[rgb(0_0_0/0.5)]"
        onMouseDown={onClose}
      />
      <div
        role="dialog"
        aria-modal
        aria-label={title}
        className={cx(
          "anim-pop relative flex max-h-[min(720px,calc(100vh-64px))] w-full flex-col overflow-hidden rounded-2xl",
          "border border-line bg-elevated shadow-pop",
        )}
        style={{ maxWidth: width }}
      >
        <div className="flex items-start justify-between gap-4 border-b border-line px-6 pt-5 pb-4">
          <div>
            <h2 className="text-[16px] font-semibold tracking-[-0.015em] text-fg">{title}</h2>
            {subtitle && <p className="mt-0.5 text-[12.5px] text-fg-3">{subtitle}</p>}
          </div>
          <IconButton icon={X} label="Close" shortcut="Esc" onClick={onClose} className="-mt-0.5 -mr-2" />
        </div>
        <div className="scroll-y min-h-0 flex-1">{children}</div>
        {footer && <div className="border-t border-line px-6 py-3">{footer}</div>}
      </div>
    </div>
  );
}
