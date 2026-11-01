import type { Kind } from "./types";

export type View = { type: "all" } | { type: "pinned" } | { type: "kind"; kind: Kind };

export function sameView(a: View, b: View): boolean {
  return a.type === b.type && (a.type !== "kind" || (b.type === "kind" && a.kind === b.kind));
}
