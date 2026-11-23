import { useEffect, useRef, useState } from "react";
import { Check, Folder, FolderInput, FolderMinus, FolderPlus, Trash2 } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import {
  COLLECTION_COLORS,
  COLLECTION_ICONS,
  NAME_MAX,
  collectionHex,
  moveClip,
  refreshCollections,
  useCollections,
} from "../lib/collections";
import { cx } from "../lib/cx";
import { plural } from "../lib/format";
import { isAndroid } from "../lib/platform";
import { toast } from "../lib/toast";
import type { ClipSummary, Collection } from "../lib/types";
import { Modal } from "./Modal";
import { Button } from "./ui";

/** A collection's emoji, or a folder in its color. */
export function CollectionIcon({
  collection,
  size = 15,
}: {
  collection: Pick<Collection, "icon" | "color"> | null;
  size?: number;
}) {
  if (collection?.icon) {
    return (
      <span aria-hidden className="leading-none" style={{ fontSize: Math.round(size * 0.95) }}>
        {collection.icon}
      </span>
    );
  }
  const hex = collectionHex(collection?.color);
  return (
    <Folder
      aria-hidden
      size={size}
      strokeWidth={1.9}
      className={hex ? undefined : "text-fg-3"}
      style={hex ? { color: hex, fill: `color-mix(in oklab, ${hex} 22%, transparent)` } : undefined}
    />
  );
}

/** New / edit collection. Deleting asks first and says the clips stay. */
export function CollectionDialog({
  collection,
  onClose,
  onSaved,
  onDeleted,
}: {
  /** Edit this one; omitted = create. */
  collection?: Collection;
  onClose: () => void;
  onSaved?: (c: Collection) => void;
  onDeleted?: (c: Collection) => void;
}) {
  const editing = !!collection;
  const [name, setName] = useState(collection?.name ?? "");
  const [icon, setIcon] = useState<string | null>(collection?.icon ?? null);
  const [color, setColor] = useState<string | null>(collection?.color ?? null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    // Phones: don't pop the keyboard over the picker when editing.
    if (!isAndroid || !editing) field.current?.focus();
  }, [editing]);

  const trimmed = name.trim();
  const unchanged =
    editing && trimmed === collection.name && icon === collection.icon && color === collection.color;

  const save = () => {
    if (!trimmed) {
      setError("Give the collection a name");
      field.current?.focus();
      return;
    }
    if (unchanged) return onClose();
    setBusy(true);
    const req = editing
      ? api.updateCollection(collection.id, trimmed, icon, color)
      : api.createCollection(trimmed, icon, color);
    req
      .then((c) => {
        void refreshCollections();
        toast(editing ? "Collection updated" : `Created “${c.name}”`);
        onSaved?.(c);
        onClose();
      })
      .catch((e) => {
        setError(errorMessage(e));
        setBusy(false);
      });
  };

  const remove = () => {
    if (!collection) return;
    setBusy(true);
    api
      .deleteCollection(collection.id)
      .then(() => {
        void refreshCollections();
        toast(`Deleted “${collection.name}”`);
        onDeleted?.(collection);
        onClose();
      })
      .catch((e) => {
        setError(errorMessage(e));
        setBusy(false);
      });
  };

  const footer = confirmDelete ? (
    <div className="anim-fade flex flex-wrap items-center gap-2">
      <p className="min-w-0 flex-1 text-[12.5px] text-fg-2">
        Delete <span className="font-medium text-fg">“{collection?.name}”</span>?{" "}
        {!collection || collection.count === 0
          ? "It's empty."
          : collection.count === 1
            ? "Its 1 clip stays in your history."
            : `Its ${plural(collection.count, "clip")} stay in your history.`}
      </p>
      <Button variant="ghost" onClick={() => setConfirmDelete(false)} disabled={busy}>
        Cancel
      </Button>
      <Button variant="primary" className="!bg-danger" onClick={remove} disabled={busy}>
        Delete
      </Button>
    </div>
  ) : (
    <div className="flex items-center gap-2">
      {editing && (
        <Button variant="danger" icon={Trash2} onClick={() => setConfirmDelete(true)} disabled={busy}>
          Delete
        </Button>
      )}
      <div className="flex-1" />
      <Button variant="ghost" onClick={onClose} disabled={busy}>
        Cancel
      </Button>
      <Button variant="primary" onClick={save} disabled={busy || !trimmed}>
        {editing ? "Save" : "Create"}
      </Button>
    </div>
  );

  return (
    <Modal
      title={editing ? "Edit collection" : "New collection"}
      subtitle={editing ? undefined : "Keep clips you want to find again. Clips in a collection are never cleaned up."}
      onClose={onClose}
      width={440}
      footer={footer}
    >
      <form
        className="space-y-5 px-6 py-5"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <div>
          <label htmlFor="collection-name" className="mb-1.5 block text-[12px] font-medium text-fg-2">
            Name
          </label>
          <div
            className={cx(
              "flex h-10 items-center gap-2.5 rounded-lg border bg-panel px-3 shadow-soft",
              error ? "border-danger" : "border-line focus-within:border-accent",
            )}
          >
            <span className="grid w-5 shrink-0 place-items-center">
              <CollectionIcon collection={{ icon, color }} size={17} />
            </span>
            <input
              id="collection-name"
              ref={field}
              value={name}
              maxLength={NAME_MAX}
              onChange={(e) => {
                setName(e.target.value);
                setError(null);
              }}
              placeholder="Work, Recipes, Project X…"
              autoComplete="off"
              aria-invalid={!!error}
              aria-describedby={error ? "collection-error" : undefined}
              className="min-w-0 flex-1 bg-transparent text-[13.5px] text-fg outline-none placeholder:text-fg-4"
            />
            {name.length > NAME_MAX - 10 && (
              <span className="text-[11px] text-fg-4 tabular-nums">{NAME_MAX - name.length}</span>
            )}
          </div>
          {error && (
            <p id="collection-error" role="alert" className="anim-fade mt-1.5 text-[12px] text-danger">
              {error}
            </p>
          )}
        </div>

        <fieldset>
          <legend className="mb-1.5 text-[12px] font-medium text-fg-2">Icon</legend>
          <div role="radiogroup" aria-label="Icon" className="grid grid-cols-7 gap-1 sm:grid-cols-[repeat(11,minmax(0,1fr))]">
            <Swatch on={icon == null} label="No icon, use a folder" onPick={() => setIcon(null)}>
              <CollectionIcon collection={{ icon: null, color }} size={16} />
            </Swatch>
            {COLLECTION_ICONS.map((e) => (
              <Swatch key={e} on={icon === e} label={e} onPick={() => setIcon(e)}>
                <span className="text-[17px] leading-none">{e}</span>
              </Swatch>
            ))}
          </div>
        </fieldset>

        <fieldset>
          <legend className="mb-1.5 text-[12px] font-medium text-fg-2">Color</legend>
          <div role="radiogroup" aria-label="Color" className="flex flex-wrap gap-2">
            <ColorDot on={color == null} label="Default" onPick={() => setColor(null)} />
            {COLLECTION_COLORS.map((c) => (
              <ColorDot key={c.key} hex={c.hex} on={color === c.key} label={c.label} onPick={() => setColor(c.key)} />
            ))}
          </div>
        </fieldset>
        {/* Enter in the name field submits. */}
        <button type="submit" hidden aria-hidden tabIndex={-1} />
      </form>
    </Modal>
  );
}

function Swatch({
  on,
  label,
  onPick,
  children,
}: {
  on: boolean;
  label: string;
  onPick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={on}
      aria-label={label}
      title={label}
      onClick={onPick}
      className={cx(
        "grid aspect-square place-items-center rounded-lg transition-colors duration-150",
        on ? "bg-accent-soft ring-2 ring-accent" : "hover:bg-hover",
      )}
    >
      {children}
    </button>
  );
}

function ColorDot({ hex, on, label, onPick }: { hex?: string; on: boolean; label: string; onPick: () => void }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={on}
      aria-label={label}
      title={label}
      onClick={onPick}
      className={cx(
        "grid h-7 w-7 place-items-center rounded-full transition-transform duration-150 hover:scale-110",
        on && "ring-2 ring-[var(--fg)] ring-offset-2 ring-offset-[var(--elevated)]",
        !hex && "border border-dashed border-line-strong",
      )}
      style={hex ? { background: hex } : undefined}
    >
      {on && <Check size={13} strokeWidth={3} className={hex ? "text-white" : "text-fg-2"} />}
    </button>
  );
}

/** "Save to collection": a button with a menu of collections (a sheet on phones). */
export function CollectionMenu({
  clip,
  onChanged,
  compact,
}: {
  clip: ClipSummary;
  onChanged?: () => void;
  /** Icon-only trigger (the clip panel header). */
  compact?: boolean;
}) {
  const collections = useCollections();
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const current = collections.find((c) => c.id === clip.collection_id) ?? null;

  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent | TouchEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", down);
    document.addEventListener("touchstart", down);
    window.addEventListener("keydown", key, true);
    // Focus the current (or first) item for keyboard users.
    requestAnimationFrame(() =>
      wrap.current?.querySelector<HTMLElement>('[role="menuitemradio"][aria-checked="true"], [role="menuitemradio"]')?.focus(),
    );
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("touchstart", down);
      window.removeEventListener("keydown", key, true);
    };
  }, [open]);

  const pick = (to: Collection | null) => {
    setOpen(false);
    moveClip(clip, to, collections, onChanged);
  };

  /** ↑/↓ between items. */
  const onMenuKey = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    e.stopPropagation();
    const items = Array.from(wrap.current?.querySelectorAll<HTMLElement>('[role^="menuitem"]') ?? []);
    const i = items.indexOf(document.activeElement as HTMLElement);
    items[(i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
  };

  const label = current ? `In “${current.name}” · move` : "Save to collection";
  const items = (
    <>
      {collections.length === 0 && (
        <p className="px-3 pt-2 pb-1 text-[12px] text-fg-3">No collections yet. Make one to keep clips together.</p>
      )}
      {collections.map((c) => {
        const on = c.id === current?.id;
        return (
          <button
            key={c.id}
            role="menuitemradio"
            aria-checked={on}
            onClick={() => pick(on ? null : c)}
            className={cx(
              "flex w-full items-center gap-2.5 rounded-lg px-2.5 text-left font-medium transition-colors hover:bg-hover focus-visible:bg-hover focus-visible:outline-none",
              isAndroid ? "h-11 text-[14px]" : "h-8 text-[12.5px]",
              on ? "text-fg" : "text-fg-2 hover:text-fg",
            )}
          >
            <span className="grid w-5 shrink-0 place-items-center">
              <CollectionIcon collection={c} size={isAndroid ? 17 : 15} />
            </span>
            <span className="min-w-0 flex-1 truncate">{c.name}</span>
            {on ? (
              <Check size={14} strokeWidth={2.4} className="shrink-0 text-accent-text" />
            ) : (
              <span className="text-[11px] text-fg-4 tabular-nums">{c.count || ""}</span>
            )}
          </button>
        );
      })}
      <div className="my-1 border-t border-line" />
      {current && (
        <button
          role="menuitem"
          onClick={() => pick(null)}
          className={cx(
            "flex w-full items-center gap-2.5 rounded-lg px-2.5 text-left font-medium text-fg-2 transition-colors hover:bg-hover hover:text-fg focus-visible:bg-hover focus-visible:outline-none",
            isAndroid ? "h-11 text-[14px]" : "h-8 text-[12.5px]",
          )}
        >
          <FolderMinus size={15} strokeWidth={1.9} className="shrink-0 text-fg-3" />
          Remove from “{current.name}”
        </button>
      )}
      <button
        role="menuitem"
        onClick={() => {
          setOpen(false);
          setCreating(true);
        }}
        className={cx(
          "flex w-full items-center gap-2.5 rounded-lg px-2.5 text-left font-medium text-accent-text transition-colors hover:bg-hover focus-visible:bg-hover focus-visible:outline-none",
          isAndroid ? "h-11 text-[14px]" : "h-8 text-[12.5px]",
        )}
      >
        <FolderPlus size={15} strokeWidth={1.9} className="shrink-0" />
        New collection…
      </button>
    </>
  );

  return (
    <div ref={wrap} className="relative" onKeyDown={onMenuKey}>
      {compact ? (
        <button
          aria-label={label}
          aria-haspopup="menu"
          aria-expanded={open}
          title={label}
          onClick={() => setOpen((v) => !v)}
          className={cx(
            "grid h-8 w-8 place-items-center rounded-lg transition-colors hover:bg-hover",
            current || open ? "text-fg" : "text-fg-3 hover:text-fg",
          )}
        >
          {current ? <CollectionIcon collection={current} size={16} /> : <FolderInput size={17} strokeWidth={2} />}
        </button>
      ) : (
        <Button
          icon={current ? undefined : FolderInput}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
          className="max-w-full"
        >
          {current && <CollectionIcon collection={current} size={14} />}
          <span className="truncate">{current ? current.name : "Save to collection"}</span>
        </Button>
      )}

      {open &&
        (isAndroid ? (
          <div className="fixed inset-0 z-50">
            <div className="anim-fade absolute inset-0 bg-[rgb(0_0_0/0.4)]" onClick={() => setOpen(false)} />
            <div
              role="menu"
              aria-label="Save to collection"
              className="anim-sheet absolute inset-x-0 bottom-0 max-h-[70vh] overflow-y-auto rounded-t-2xl border-t border-line bg-elevated px-2 pt-2 pb-[max(12px,env(safe-area-inset-bottom))] shadow-pop"
            >
              <div className="mx-auto mb-2 h-1 w-9 rounded-full bg-line-strong" />
              <div className="px-2.5 pb-1.5 text-[13px] font-semibold text-fg">Save to collection</div>
              {items}
            </div>
          </div>
        ) : (
          <div
            role="menu"
            aria-label="Save to collection"
            className="anim-pop absolute top-full right-0 z-40 mt-1 max-h-[340px] w-[240px] overflow-y-auto rounded-xl border border-line bg-elevated p-1 shadow-pop"
          >
            {items}
          </div>
        ))}

      {creating && (
        <CollectionDialog
          onClose={() => setCreating(false)}
          onSaved={(c) => moveClip(clip, c, [...collections, c], onChanged)}
        />
      )}
    </div>
  );
}
