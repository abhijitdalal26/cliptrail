import { useEffect, useState } from "react";
import { formatDistanceToNowStrict } from "date-fns";
import { CircleCheck, Globe, Lock, Plus, TriangleAlert } from "lucide-react";
import { errorMessage } from "../lib/api";
import { cx } from "../lib/cx";
import { setSyncView, syncApi, type RelayHello, type RelayView, type SyncView } from "../lib/sync";
import { Button, Checkbox } from "./ui";

type Tone = "ok" | "idle" | "warn" | "bad";

/** What a relay's state means to someone who doesn't care about the protocol. */
function relayState(r: RelayView, syncOn: boolean): { tone: Tone; text: string } {
  if (!syncOn) return { tone: "idle", text: "Sync is off" };
  switch (r.state) {
    case "connected":
      return { tone: "ok", text: "Connected over the internet" };
    case "standby":
      return { tone: "ok", text: "Using home network · relay on standby" };
    case "connecting":
      return { tone: "idle", text: "Connecting…" };
    case "denied":
      return { tone: "bad", text: "This device was removed from the relay" };
    case "offline":
      return { tone: "warn", text: "Can't reach relay" };
    default:
      return r.enrolled ? { tone: "idle", text: "Not running" } : { tone: "idle", text: "Setting up…" };
  }
}

function ago(t: number | null): string | null {
  if (!t) return null;
  if (Date.now() - t < 45_000) return "just now";
  return `${formatDistanceToNowStrict(t)} ago`;
}

function host(url: string): string {
  try {
    const u = new URL(url);
    return u.host + (u.pathname === "/" ? "" : u.pathname);
  } catch {
    return url;
  }
}

/**
 * "Sync anywhere": off-LAN sync through a self-hosted relay. Rows only; the
 * caller puts them in its card (`divide-y` container). `mobile` uses larger
 * touch targets and stacks the form.
 */
export function RelayCard({ view, mobile = false }: { view: SyncView; mobile?: boolean }) {
  const relays = view.status.relays ?? [];
  const [adding, setAdding] = useState(false);
  const showForm = adding || relays.length === 0;

  return (
    <>
      <div className={cx(mobile ? "px-4 py-4" : "px-4 py-3.5")}>
        <div className="flex items-start gap-3">
          <span
            className={cx(
              "grid shrink-0 place-items-center rounded-xl bg-accent-soft text-accent-text",
              mobile ? "h-10 w-10" : "h-8 w-8 rounded-lg",
            )}
          >
            <Globe size={mobile ? 19 : 15} strokeWidth={2} />
          </span>
          <div className="min-w-0 flex-1">
            <div className={cx("font-medium text-fg", mobile ? "text-[14px] font-semibold" : "text-[13px]")}>
              Sync anywhere (relay)
            </div>
            <div className={cx("mt-0.5 leading-[1.45] text-fg-3", mobile ? "text-[12.5px]" : "text-[12px]")}>
              Keep syncing when your devices aren't on the same network, through a relay you run or trust. The relay
              only ever stores end-to-end-encrypted data it can't read. Your other devices pick it up the next time
              they're on the same Wi-Fi as this one.
            </div>
          </div>
        </div>

        {relays.length > 0 && (
          <div className={cx("mt-3 space-y-1", !mobile && "pl-11")}>
            {relays.map((r) => (
              <RelayRow key={r.url} relay={r} syncOn={view.enabled} mobile={mobile} />
            ))}
          </div>
        )}

        {showForm ? (
          <AddRelayForm
            mobile={mobile}
            onDone={() => setAdding(false)}
            onCancel={relays.length > 0 ? () => setAdding(false) : undefined}
          />
        ) : (
          <div className={cx("mt-2", !mobile && "pl-11")}>
            <Button size="sm" variant="ghost" icon={Plus} className="-ml-2" onClick={() => setAdding(true)}>
              Add another relay
            </Button>
          </div>
        )}
      </div>
    </>
  );
}

function Dot({ tone }: { tone: Tone }) {
  return (
    <span
      className={cx(
        "h-[7px] w-[7px] shrink-0 rounded-full",
        tone === "ok" && "bg-success",
        tone === "idle" && "bg-fg-4",
        tone === "warn" && "bg-[#f59e0b]",
        tone === "bad" && "bg-danger",
      )}
    />
  );
}

function RelayRow({ relay: r, syncOn, mobile }: { relay: RelayView; syncOn: boolean; mobile: boolean }) {
  const [confirm, setConfirm] = useState(false);
  const [purge, setPurge] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const st = relayState(r, syncOn);
  const last = ago(r.last_ok);

  const remove = async () => {
    setBusy(true);
    try {
      setSyncView(await syncApi.relayRemove(r.url, purge));
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <div className="rounded-lg border border-line bg-panel-2 px-3 py-2.5">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="code truncate text-[12.5px] font-medium text-fg" title={r.url}>
            {host(r.url)}
          </div>
          <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-[11.5px] font-medium">
            <Dot tone={st.tone} />
            <span
              className={cx(
                "truncate",
                st.tone === "warn" && "text-[#b45309] dark:text-[#fbbf24]",
                st.tone === "bad" && "text-danger",
                (st.tone === "ok" || st.tone === "idle") && "text-fg-2",
              )}
            >
              {st.text}
              {last && st.tone !== "bad" && <span className="font-normal text-fg-3"> · last sync {last}</span>}
            </span>
          </div>
          {r.last_error && st.tone !== "ok" && (
            <div className="mt-0.5 truncate text-[11px] text-fg-3" title={r.last_error}>
              {r.last_error}
            </div>
          )}
        </div>
        {!confirm && (
          <Button size="sm" variant="ghost" onClick={() => setConfirm(true)}>
            Remove
          </Button>
        )}
      </div>
      {confirm && (
        <div className="anim-drop mt-2 border-t border-line pt-2">
          <Checkbox
            checked={purge}
            onChange={setPurge}
            label="Also delete my data from the relay"
            hint="Removes this space's encrypted clips from the relay's storage. Clips on your devices stay."
            disabled={busy}
          />
          <div className={cx("mt-1.5 flex gap-1.5", mobile ? "justify-stretch" : "justify-end")}>
            <Button size="sm" disabled={busy} className={cx(mobile && "flex-1")} onClick={() => setConfirm(false)}>
              Cancel
            </Button>
            <Button
              size="sm"
              variant="danger"
              disabled={busy}
              className={cx("bg-[color-mix(in_oklab,var(--danger)_10%,transparent)]", mobile && "flex-1")}
              onClick={() => void remove()}
            >
              {busy ? "Removing…" : "Remove relay"}
            </Button>
          </div>
          <div className="mt-1.5 text-[11.5px] leading-[1.45] text-fg-3">
            Your other devices stop using it too, the next time they sync.
          </div>
        </div>
      )}
      {error && (
        <div className="anim-fade mt-1.5 flex items-center gap-1.5 text-[12px] font-medium text-danger">
          <TriangleAlert size={12.5} className="shrink-0" />
          {error}
        </div>
      )}
    </div>
  );
}

function AddRelayForm({ mobile, onDone, onCancel }: { mobile: boolean; onDone: () => void; onCancel?: () => void }) {
  const [url, setUrl] = useState("");
  const [invite, setInvite] = useState("");
  const [hello, setHello] = useState<RelayHello | null>(null);
  const [busy, setBusy] = useState<"test" | "add" | null>(null);
  const [error, setError] = useState<string | null>(null);

  // A different URL needs a new test.
  useEffect(() => {
    setHello(null);
    setError(null);
  }, [url]);

  const needsInvite = hello?.registration === "invite";
  const closed = hello?.registration === "closed";
  const insecure = /^http:\/\//i.test(url.trim());

  const test = async () => {
    setBusy("test");
    setError(null);
    try {
      setHello(await syncApi.relayCheck(url.trim()));
    } catch (e) {
      setHello(null);
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const add = async () => {
    setBusy("add");
    setError(null);
    try {
      setSyncView(await syncApi.relayAdd(url.trim(), needsInvite ? invite.trim() : null));
      setUrl("");
      setInvite("");
      onDone();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const canAdd = !!hello && !closed && (!needsInvite || invite.trim().length > 0);
  const input =
    "code min-w-0 rounded-lg border border-line bg-panel px-2.5 text-fg shadow-soft outline-none placeholder:font-sans placeholder:text-fg-4 focus-visible:border-accent";

  return (
    <div className={cx("mt-3", !mobile && "pl-11")}>
      <form
        className={cx("flex gap-1.5", mobile && "flex-col")}
        onSubmit={(e) => {
          e.preventDefault();
          if (canAdd) void add();
          else if (url.trim()) void test();
        }}
      >
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://relay.example.com"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          inputMode="url"
          aria-label="Relay address"
          className={cx(input, "flex-1", mobile ? "h-10 text-[13px]" : "h-8 text-[12.5px]")}
        />
        <div className="flex gap-1.5">
          {onCancel && (
            <Button type="button" size={mobile ? "md" : "sm"} variant="ghost" onClick={onCancel} className={cx(mobile ? "h-10 flex-1" : "h-8")}>
              Cancel
            </Button>
          )}
          {canAdd ? (
            <Button type="submit" size={mobile ? "md" : "sm"} variant="primary" icon={Plus} disabled={busy != null} className={cx(mobile ? "h-10 flex-1" : "h-8")}>
              {busy === "add" ? "Adding…" : "Add relay"}
            </Button>
          ) : (
            <Button
              type="submit"
              size={mobile ? "md" : "sm"}
              variant={hello ? "secondary" : "primary"}
              disabled={!url.trim() || busy != null}
              className={cx(mobile ? "h-10 flex-1" : "h-8")}
            >
              {busy === "test" ? "Testing…" : "Test"}
            </Button>
          )}
        </div>
      </form>

      {hello && (
        <div className="anim-fade mt-2 flex items-start gap-1.5 text-[12px] leading-[1.45]">
          {closed ? (
            <TriangleAlert size={13} className="mt-[2px] shrink-0 text-danger" />
          ) : (
            <CircleCheck size={13} className="mt-[2px] shrink-0 text-success" />
          )}
          <span className="text-fg-2">
            ClipTrail relay {hello.version} ·{" "}
            {closed
              ? "not accepting new spaces"
              : needsInvite
                ? "needs an invite code from its operator"
                : "open to new spaces"}
            {hello.limits.retention_days > 0 && !closed && (
              <span className="text-fg-3"> · keeps data {hello.limits.retention_days} days</span>
            )}
          </span>
        </div>
      )}
      {needsInvite && (
        <input
          value={invite}
          onChange={(e) => setInvite(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && canAdd && busy == null) {
              e.preventDefault();
              void add();
            }
          }}
          placeholder="Invite code"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          aria-label="Invite code"
          className={cx(input, "anim-drop mt-2 w-full", mobile ? "h-10 text-[13px]" : "h-8 max-w-[260px] text-[12.5px]")}
        />
      )}
      {error && (
        <div className="anim-fade mt-2 flex items-start gap-1.5 text-[12px] font-medium text-danger">
          <TriangleAlert size={12.5} className="mt-[2px] shrink-0" />
          <span>{error}</span>
        </div>
      )}
      <div className="mt-2 flex items-start gap-1.5 text-[11.5px] leading-[1.45] text-fg-3">
        <Lock size={12} className="mt-[2px] shrink-0" />
        <span>
          {insecure
            ? "Plain http:// only works for a relay on your own network; use https:// for one on the internet."
            : "Needs an https:// address. Self-host one with the Docker kit in the ClipTrail repo."}
        </span>
      </div>
    </div>
  );
}
