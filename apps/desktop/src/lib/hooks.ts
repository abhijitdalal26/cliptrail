import { useEffect, useLayoutEffect, useRef, useState } from "react";

export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

/** Stable callback that always sees the latest props/state. */
export function useEvent<A extends unknown[], R>(fn: (...args: A) => R): (...args: A) => R {
  const ref = useRef(fn);
  useLayoutEffect(() => {
    ref.current = fn;
  });
  const stable = useRef((...args: A) => ref.current(...args));
  return stable.current;
}

/** Re-render every `ms` so relative times stay fresh. */
export function useTick(ms: number): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setN((x) => x + 1), ms);
    return () => clearInterval(t);
  }, [ms]);
  return n;
}

export const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);
export const MOD = isMac ? "⌘" : "Ctrl";

/** True when typing in a text field (so single-key shortcuts don't fire). */
export function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  if (t.isContentEditable) return true;
  if (t instanceof HTMLTextAreaElement) return true;
  if (t instanceof HTMLInputElement) {
    return !["checkbox", "radio", "button", "range"].includes(t.type);
  }
  return false;
}
