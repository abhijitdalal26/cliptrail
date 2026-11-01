import { useSyncExternalStore } from "react";

export interface Toast {
  id: number;
  message: string;
  tone?: "default" | "error";
  action?: { label: string; run: () => void };
  /** ms; default 2200 (5000 with an action). */
  duration?: number;
}

let toasts: Toast[] = [];
let next = 1;
const subs = new Set<() => void>();
const emit = () => subs.forEach((f) => f());

export function toast(t: Omit<Toast, "id"> | string): number {
  const item: Toast = typeof t === "string" ? { id: next++, message: t } : { ...t, id: next++ };
  toasts = [...toasts.slice(-2), item];
  emit();
  const ms = item.duration ?? (item.action ? 5000 : 2200);
  setTimeout(() => dismiss(item.id), ms);
  return item.id;
}

export function dismiss(id: number) {
  const before = toasts.length;
  toasts = toasts.filter((t) => t.id !== id);
  if (toasts.length !== before) emit();
}

export function useToasts(): Toast[] {
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => toasts,
  );
}
