import { useEffect, useState } from "react";
import { Bot, Check, ChevronDown, Copy } from "lucide-react";
import { api, errorMessage, type AiClient } from "../lib/api";
import { relative } from "../lib/format";
import { cx } from "../lib/cx";
import { Button, Switch } from "../components/ui";
import { Row, Section } from "./SettingsDialog";

const DOCS = "https://abhijitdalal26.github.io/cliptrail/docs/ai-assistants.html";

/** Settings → AI assistants: one switch connects every AI app found here; "Choose apps" for per-app control. */
export function AssistantsSection() {
  const [clients, setClients] = useState<AiClient[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [choose, setChoose] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    void api.aiClients().then(setClients).catch((e) => setError(errorMessage(e)));
  }, []);

  const run = (f: () => Promise<AiClient[]>) => {
    setBusy(true);
    setError(null);
    f()
      .then(setClients)
      .catch((e) => {
        setError(errorMessage(e));
        void api.aiClients().then(setClients).catch(() => undefined);
      })
      .finally(() => setBusy(false));
  };

  const copyConfig = () => {
    void api
      .aiConfigSnippet()
      .then((s) => navigator.clipboard.writeText(s))
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1600);
      })
      .catch((e) => setError(errorMessage(e)));
  };

  // Only apps that are really here (or still connected from before) are listed.
  const list = (clients ?? []).filter((c) => c.installed || c.connected);
  const connected = list.filter((c) => c.connected);
  const found = list.filter((c) => c.installed);
  const names = (cs: AiClient[]) => cs.map((c) => c.name).join(", ");
  const lastUse = connected
    .filter((c) => c.last_used)
    .sort((a, b) => (b.last_used ?? 0) - (a.last_used ?? 0))[0];
  const status = !clients
    ? "Looking for AI apps…"
    : connected.length > 0
      ? `Connected: ${names(connected)}. ` +
        (lastUse
          ? `${lastUse.name} last used it ${relative(lastUse.last_used!)}.`
          : "Restart an app if it was open; it shows here once it has used ClipTrail.")
      : found.length > 0
        ? `Found on this computer: ${names(found)}.`
        : "No AI apps found on this computer. Use Copy config for any MCP app.";

  return (
    <Section title="AI assistants">
      <Row
        title="Let AI assistants search my history"
        desc={
          <>
            {status}
            <span className="mt-1 flex items-start gap-1.5">
              <Bot size={13} className="mt-[2px] shrink-0 text-accent-text" />
              <span>
                Runs only on this computer; ClipTrail talks to no cloud. Sensitive clips are never shared.{" "}
                <button
                  className="text-accent-text hover:underline"
                  onClick={() => void api.openUrl(DOCS).catch(() => undefined)}
                >
                  How it works
                </button>
              </span>
            </span>
          </>
        }
        error={error}
      >
        <Switch
          label="Let AI assistants search my history"
          checked={connected.length > 0}
          disabled={busy || !clients || (connected.length === 0 && found.length === 0)}
          onChange={(on) => run(on ? api.aiConnectAll : api.aiDisconnectAll)}
        />
      </Row>

      <div className="flex items-center justify-between px-4 py-2.5">
        <button
          onClick={() => setChoose((v) => !v)}
          className="flex items-center gap-1.5 text-[12px] font-medium text-fg-3 hover:text-fg"
        >
          <ChevronDown size={13} className={cx("transition-transform duration-150", choose && "rotate-180")} />
          Choose apps
        </button>
        <Button size="sm" variant="ghost" icon={copied ? Check : Copy} onClick={copyConfig}>
          {copied ? "Copied" : "Copy config"}
        </Button>
      </div>

      {choose &&
        list.map((c) => (
          <Row
            key={c.id}
            title={c.name}
            desc={
              c.connected
                ? c.last_used
                  ? `Connected · used ${relative(c.last_used)}`
                  : "Connected · not used yet (restart the app)"
                : "Found on this computer"
            }
          >
            {c.connected ? (
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(() => api.aiDisconnect(c.id))}>
                Disconnect
              </Button>
            ) : (
              <Button size="sm" disabled={busy} onClick={() => run(() => api.aiConnect(c.id))}>
                Connect
              </Button>
            )}
          </Row>
        ))}
    </Section>
  );
}
