// The user's collections (folders for clips), shared by the sidebar, the
// clip panel, the cards and the phone's chips. Loaded once and refreshed
// whenever clips change (that covers edits from synced devices too).

import { useSyncExternalStore } from "react";
import { api, errorMessage, onClipsChanged } from "./api";
import { toast } from "./toast";
import type { ClipSummary, Collection } from "./types";

/** Accent colors a collection can have. Keys are stored; hexes only drawn. */
export const COLLECTION_COLORS: { key: string; label: string; hex: string }[] = [
  { key: "blue", label: "Blue", hex: "#3b82f6" },
  { key: "teal", label: "Teal", hex: "#14b8a6" },
  { key: "green", label: "Green", hex: "#22c55e" },
  { key: "amber", label: "Amber", hex: "#f59e0b" },
  { key: "orange", label: "Orange", hex: "#f97316" },
  { key: "red", label: "Red", hex: "#ef4444" },
  { key: "pink", label: "Pink", hex: "#ec4899" },
  { key: "purple", label: "Purple", hex: "#8b5cf6" },
  { key: "slate", label: "Slate", hex: "#64748b" },
];

/** Emoji offered as collection icons. */
export const COLLECTION_ICONS = [
  "💼", "🏠", "📚", "💡", "🧑‍💻", "🛠️", "📝", "🔗",
  "🛒", "✈️", "🍳", "💰", "🎨", "📷", "🎵", "🎮",
  "❤️", "⭐", "🔥", "📦",
];

export const NAME_MAX = 40;

/** The hex of a collection's color, or null for the default look. */
export function collectionHex(color: string | null | undefined): string | null {
  return COLLECTION_COLORS.find((c) => c.key === color)?.hex ?? null;
}

let current: Collection[] | null = null;
const listeners = new Set<() => void>();
let started = false;
let timer: ReturnType<typeof setTimeout> | undefined;

function emit() {
  listeners.forEach((l) => l());
}

/** Reload the list now (after an edit made here). */
export function refreshCollections(): Promise<void> {
  return api
    .listCollections()
    .then((list) => {
      current = list;
      emit();
    })
    .catch(() => undefined);
}

function subscribe(cb: () => void) {
  listeners.add(cb);
  if (!started) {
    started = true;
    void refreshCollections();
    onClipsChanged(() => {
      clearTimeout(timer);
      timer = setTimeout(() => void refreshCollections(), 150);
    });
  }
  return () => {
    listeners.delete(cb);
  };
}

const EMPTY: Collection[] = [];

/** Collections in the user's order (empty until loaded). */
export function useCollections(): Collection[] {
  return useSyncExternalStore(subscribe, () => current ?? EMPTY);
}

/** True once the list has loaded (an empty list then means there are none). */
export function collectionsLoaded(): boolean {
  return current !== null;
}

/** Move `id` to position `to` (in the list without it). */
export function moveCollection(list: readonly Collection[], id: string, to: number): Collection[] {
  const item = list.find((c) => c.id === id);
  if (!item) return [...list];
  const rest = list.filter((c) => c.id !== id);
  const at = Math.max(0, Math.min(to, rest.length));
  return [...rest.slice(0, at), item, ...rest.slice(at)];
}

/** Save a new order, showing it right away. */
export function reorderCollections(next: Collection[]): Promise<void> {
  current = next;
  emit();
  return api.reorderCollections(next.map((c) => c.id)).finally(() => void refreshCollections());
}

/** Drag-and-drop payload type for a clip dragged onto a collection. */
export const CLIP_DRAG_TYPE = "application/x-cliptrail-clip";

/** Put a clip in a collection (null: take it out), with an Undo toast. */
export function moveClip(
  clip: Pick<ClipSummary, "id" | "collection_id">,
  to: Collection | null,
  collections: Collection[],
  onChanged?: () => void,
) {
  const from = clip.collection_id ?? null;
  if ((to?.id ?? null) === from) return;
  const before = collections.find((c) => c.id === from) ?? null;
  api
    .setClipCollection(clip.id, to?.id ?? null)
    .then(() => {
      onChanged?.();
      void refreshCollections();
      toast({
        message: to ? `Saved to “${to.name}”` : `Removed from “${before?.name ?? "collection"}”`,
        action: {
          label: "Undo",
          run: () =>
            void api
              .setClipCollection(clip.id, from)
              .then(() => {
                onChanged?.();
                void refreshCollections();
              })
              .catch((e) => toast({ message: errorMessage(e), tone: "error" })),
        },
      });
    })
    .catch((e) => toast({ message: errorMessage(e), tone: "error" }));
}
