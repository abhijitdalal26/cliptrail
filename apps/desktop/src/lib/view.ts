import type { Kind } from "./types";

export type View =
  | { type: "dashboard" }
  | { type: "all" }
  | { type: "favorites" }
  | { type: "recent" }
  | { type: "trash" }
  | { type: "kind"; kind: Kind };

/** "Recent" covers this many days. */
export const RECENT_DAYS = 7;

export function sameView(a: View, b: View): boolean {
  return a.type === b.type && (a.type !== "kind" || (b.type === "kind" && a.kind === b.kind));
}
