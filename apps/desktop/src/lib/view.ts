import type { Kind } from "./types";

export type View =
  | { type: "dashboard" }
  | { type: "favorites" }
  | { type: "recent" }
  | { type: "trash" }
  | { type: "kind"; kind: Kind }
  | { type: "collection"; id: string };

/** "Recent" covers this many days. */
export const RECENT_DAYS = 7;

export function sameView(a: View, b: View): boolean {
  if (a.type === "kind") return b.type === "kind" && a.kind === b.kind;
  if (a.type === "collection") return b.type === "collection" && a.id === b.id;
  return a.type === b.type;
}
