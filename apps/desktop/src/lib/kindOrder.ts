// The user's order of the sidebar categories, kept in settings.

import { useEffect, useState } from "react";
import { api, listen } from "./api";
import { ALL_KINDS, type Kind, type SettingsView } from "./types";

/** Kinds in the saved order; kinds the order doesn't mention follow in the default order. */
export function orderedKinds(order: readonly string[] | undefined): Kind[] {
  const known = (order ?? []).filter((k): k is Kind => (ALL_KINDS as string[]).includes(k));
  return [...known, ...ALL_KINDS.filter((k) => !known.includes(k))];
}

/** Move `kind` to position `to` (in the list without it). */
export function moveKind(kinds: readonly Kind[], kind: Kind, to: number): Kind[] {
  const rest = kinds.filter((k) => k !== kind);
  const at = Math.max(0, Math.min(to, rest.length));
  return [...rest.slice(0, at), kind, ...rest.slice(at)];
}

export function useKindOrder(): [Kind[], (next: Kind[]) => void] {
  const [view, setView] = useState<SettingsView | null>(null);
  useEffect(() => {
    const load = () =>
      void api
        .getSettings()
        .then(setView)
        .catch(() => undefined);
    load();
    return listen("settings-changed", load);
  }, []);
  const kinds = orderedKinds(view?.settings.kind_order);
  const save = (next: Kind[]) => {
    if (!view) return;
    const settings = { ...view.settings, kind_order: next };
    setView({ ...view, settings });
    void api
      .updateSettings(settings)
      .then(setView)
      .catch(() => undefined);
  };
  return [kinds, save];
}
