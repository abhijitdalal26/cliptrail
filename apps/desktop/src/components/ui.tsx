import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import { appHue, appName } from "../lib/apps";
import { cx } from "../lib/cx";
import { KIND_META } from "../lib/kinds";
import type { Kind } from "../lib/types";


type BtnProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
  icon?: LucideIcon;
};

export const Button = forwardRef<HTMLButtonElement, BtnProps>(function Button(
  { variant = "secondary", size = "md", icon: Icon, className, children, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      className={cx(
        "inline-flex select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-lg font-medium",
        "transition-[background-color,border-color,color,box-shadow,transform] duration-150 active:scale-[0.98]",
        "disabled:pointer-events-none disabled:opacity-45",
        size === "sm" ? "h-7 px-2.5 text-[12px]" : "h-8 px-3 text-[12.5px]",
        variant === "primary" &&
          "bg-accent text-accent-fg shadow-[inset_0_1px_0_rgb(255_255_255/0.16),0_1px_2px_rgb(0_0_0/0.12)] hover:bg-accent-hover",
        variant === "secondary" &&
          "border border-line bg-panel text-fg shadow-soft hover:border-line-strong hover:bg-panel-2",
        variant === "ghost" && "text-fg-2 hover:bg-hover hover:text-fg",
        variant === "danger" && "text-danger hover:bg-[color-mix(in_oklab,var(--danger)_10%,transparent)]",
        className,
      )}
      {...rest}
    >
      {Icon && <Icon size={size === "sm" ? 13 : 14} strokeWidth={2} className="shrink-0" />}
      {children}
    </button>
  );
});

export function IconButton({
  icon: Icon,
  label,
  active,
  className,
  size = 30,
  shortcut,
  tone,
  tooltipSide,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  icon: LucideIcon;
  label: string;
  active?: boolean;
  size?: number;
  shortcut?: string;
  tone?: "danger";
  tooltipSide?: "top" | "bottom";
}) {
  return (
    <Tooltip label={label} shortcut={shortcut} side={tooltipSide}>
      <button
        aria-label={label}
        className={cx(
          "inline-grid shrink-0 place-items-center rounded-lg transition-colors duration-150",
          active ? "bg-accent-soft text-accent-text" : "text-fg-3 hover:bg-hover hover:text-fg",
          tone === "danger" && "hover:!text-danger",
          className,
        )}
        style={{ width: size, height: size }}
        {...rest}
      >
        <Icon size={Math.round(size * 0.52)} strokeWidth={1.9} />
      </button>
    </Tooltip>
  );
}

/** CSS-only tooltip (no portal): appears below the trigger after a short delay. */
export function Tooltip({
  label,
  shortcut,
  children,
  side = "bottom",
}: {
  label: string;
  shortcut?: string;
  children: ReactNode;
  side?: "bottom" | "top";
}) {
  return (
    <span className="group/tt relative inline-flex">
      {children}
      <span
        role="tooltip"
        className={cx(
          "pointer-events-none absolute left-1/2 z-50 flex -translate-x-1/2 items-center gap-1.5 whitespace-nowrap rounded-md",
          "bg-[#1c1c21] px-2 py-1 text-[11.5px] font-medium text-white shadow-pop dark:bg-[#2a2a31]",
          "opacity-0 transition-opacity delay-0 duration-100 group-hover/tt:opacity-100 group-hover/tt:delay-500",
          side === "bottom" ? "top-[calc(100%+6px)]" : "bottom-[calc(100%+6px)]",
        )}
      >
        {label}
        {shortcut && <span className="text-white/55">{shortcut}</span>}
      </span>
    </span>
  );
}

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={cx(
        "inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-[5px] border border-line bg-panel-2 px-1",
        "font-sans text-[10.5px] font-medium text-fg-3 shadow-[0_1px_0_var(--border)]",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

export function KindTile({ kind, size = 30, className }: { kind: Kind; size?: number; className?: string }) {
  const Icon = KIND_META[kind].icon;
  return (
    <span
      className={cx(`k-${kind} k-tile inline-grid shrink-0 place-items-center rounded-lg`, className)}
      style={{ width: size, height: size, borderRadius: size * 0.3 }}
    >
      <Icon size={Math.round(size * 0.5)} strokeWidth={2} />
    </span>
  );
}

export function KindBadge({ kind, label }: { kind: Kind; label?: string | null }) {
  return (
    <span
      className={cx(
        `k-${kind} k-tile inline-flex h-[18px] items-center rounded-[5px] px-1.5 text-[10.5px] font-semibold tracking-[0.01em]`,
      )}
    >
      {label ?? KIND_META[kind].label}
    </span>
  );
}

export function AppAvatar({ app, size = 16 }: { app: string | null; size?: number }) {
  const hue = appHue(app);
  const name = appName(app);
  return (
    <span
      aria-hidden
      className="inline-grid shrink-0 place-items-center font-semibold"
      style={{
        width: size,
        height: size,
        borderRadius: size * 0.28,
        fontSize: size * 0.56,
        color: `oklch(0.52 0.13 ${hue})`,
        background: `oklch(0.62 0.13 ${hue} / 0.16)`,
      }}
    >
      {name.charAt(0).toUpperCase()}
    </span>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cx(
        "relative h-[20px] w-[34px] shrink-0 rounded-full transition-colors duration-200 disabled:opacity-40",
        checked ? "bg-accent" : "bg-active",
      )}
    >
      <span
        className={cx(
          "absolute top-[2px] left-[2px] h-4 w-4 rounded-full bg-white shadow-[0_1px_3px_rgb(0_0_0/0.25)]",
          "transition-transform duration-200 ease-out-soft",
          checked && "translate-x-[14px]",
        )}
      />
    </button>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  size = "md",
}: {
  value: T;
  options: { value: T; label: string; icon?: LucideIcon }[];
  onChange: (v: T) => void;
  size?: "sm" | "md";
}) {
  return (
    <div className="inline-flex rounded-lg bg-hover p-0.5">
      {options.map((o) => {
        const Icon = o.icon;
        const on = o.value === value;
        return (
          <button
            key={o.value}
            onClick={() => onChange(o.value)}
            className={cx(
              "inline-flex items-center gap-1.5 rounded-md font-medium transition-all duration-150",
              size === "sm" ? "h-6 px-2 text-[11.5px]" : "h-7 px-3 text-[12px]",
              on ? "bg-elevated text-fg shadow-soft" : "text-fg-3 hover:text-fg",
            )}
          >
            {Icon && <Icon size={13} strokeWidth={2} />}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

export function Chip({
  active,
  children,
  onClick,
  icon: Icon,
  onRemove,
}: {
  active?: boolean;
  children: ReactNode;
  onClick?: () => void;
  icon?: LucideIcon;
  onRemove?: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={cx(
        "inline-flex h-7 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-[12px] font-medium transition-colors duration-150",
        active
          ? "border-transparent bg-accent-soft text-accent-text"
          : "border-line text-fg-2 hover:border-line-strong hover:bg-hover hover:text-fg",
      )}
    >
      {Icon && <Icon size={13} strokeWidth={2} />}
      {children}
      {onRemove && (
        <span
          role="button"
          aria-label="Remove filter"
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
          className="-mr-1 ml-0.5 grid h-4 w-4 place-items-center rounded-full hover:bg-active"
        >
          ×
        </span>
      )}
    </button>
  );
}
