import { useEffect, useState } from "react";
import { Trash2 } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { plural } from "../lib/format";
import { otherDevices, useSyncView } from "../lib/sync";
import { toast } from "../lib/toast";
import { Modal } from "./Modal";
import { Button, Checkbox } from "./ui";

/** Confirm + run "Clear history". Shared by the desktop and phone Settings. */
export function ClearHistoryDialog({ onClose }: { onClose: () => void }) {
  const sync = useSyncView();
  const paired = !!sync && sync.status.configured && otherDevices(sync).length > 0;
  const [keepPinned, setKeepPinned] = useState(true);
  const [everywhere, setEverywhere] = useState(false);
  const [count, setCount] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setCount(null);
    api
      .clearCount(keepPinned)
      .then((n) => live && setCount(n))
      .catch((e) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [keepPinned]);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const n = await api.clearHistory(keepPinned, paired && everywhere);
      toast(
        n === 0
          ? "Nothing to clear"
          : `Cleared ${plural(n, "clip")}${paired && everywhere ? " on all your devices" : ""}`,
      );
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  const where = paired && everywhere ? "on this device and your other devices" : "from this device";
  return (
    <Modal
      title="Clear history"
      subtitle="This can't be undone."
      width={440}
      onClose={busy ? () => undefined : onClose}
      footer={
        <div className="flex items-center justify-end gap-2">
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            icon={Trash2}
            disabled={busy || count == null || count === 0}
            onClick={() => void run()}
            className="!border-transparent !bg-danger !text-white hover:opacity-90"
          >
            {busy ? "Clearing…" : count ? `Delete ${plural(count, "clip")}` : "Delete"}
          </Button>
        </div>
      }
    >
      <div className="space-y-3 px-6 py-5">
        <p className="text-[13px] leading-[1.5] text-fg-2">
          {count == null ? (
            "Counting clips…"
          ) : count === 0 ? (
            "There is nothing to clear."
          ) : (
            <>
              <span className="font-semibold text-fg">{plural(count, "clip")}</span>{" "}
              and their images will be permanently deleted {where}.
            </>
          )}
        </p>
        <div className="space-y-1 rounded-xl border border-line bg-panel-2 px-3.5 py-2.5">
          <Checkbox label="Keep pinned clips" checked={keepPinned} onChange={setKeepPinned} disabled={busy} />
          {paired && (
            <Checkbox
              label="Also clear on my other devices"
              hint="Off: your other devices keep their copies."
              checked={everywhere}
              onChange={setEverywhere}
              disabled={busy}
            />
          )}
        </div>
        {error && <div className="anim-fade text-[12.5px] font-medium text-danger">{error}</div>}
      </div>
    </Modal>
  );
}
