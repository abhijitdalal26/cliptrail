import { useEffect, useState } from "react";
import { Bot, Check, ChevronDown, Copy } from "lucide-react";
import { api, errorMessage, type AiClient } from "../lib/api";
import { cx } from "../lib/cx";
import { Button } from "../components/ui";
import { Row, Section } from "./SettingsDialog";

const DOCS = "https://abhijitdalal26.github.io/cliptrail/docs/ai-assistants.html";

/** Settings → AI assistants: one-click MCP setup in the AI apps on this computer. */
export function AssistantsSection() {
  const [clients, setClients] = useState<AiClient[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    void api.aiClients().then(setClients).catch((e) => setError(errorMessage(e)));
  }, []);

  const run = (id: string, f: (id: string) => Promise<AiClient[]>) => {
    setBusy(id);
    setError(null);
    f(id)
      .then(setClients)
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setBusy(null));
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

  const found = (clients ?? []).filter((c) => c.installed || c.connected);
  const others = (clients ?? []).filter((c) => !c.installed && !c.connected);
  const shown = all ? [...found, ...others] : found;

  return (
    <Section title="AI assistants">
      <div className="flex gap-3 px-4 py-3.5">
        <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-accent-soft text-accent-text">
          <Bot size={16} strokeWidth={2} />
        </span>
        <div className="text-[12.5px] leading-[1.5] text-fg-3">
          <span className="font-medium text-fg">Ask your AI assistant to find what you copied.</span> It runs only on this
          computer and ClipTrail sends nothing to any cloud. Sensitive clips are never shared.{" "}
          <button className="text-accent-text hover:underline" onClick={() => void api.openUrl(DOCS).catch(() => undefined)}>
            How it works
          </button>
        </div>
      </div>

      {clients && found.length === 0 && !all && (
        <div className="px-4 py-3 text-[12.5px] text-fg-3">No AI apps found on this computer yet.</div>
      )}
      {shown.map((c) => (
        <Row
          key={c.id}
          title={c.name}
          desc={
            c.connected
              ? "Connected. Restart the app if it's open."
              : c.installed
                ? "Found on this computer"
                : "Not found on this computer"
          }
        >
          {c.connected ? (
            <Button size="sm" variant="ghost" disabled={busy === c.id} onClick={() => run(c.id, api.aiDisconnect)}>
              Disconnect
            </Button>
          ) : (
            <Button
              size="sm"
              variant={c.installed ? "primary" : "secondary"}
              disabled={busy === c.id}
              onClick={() => run(c.id, api.aiConnect)}
            >
              Connect
            </Button>
          )}
        </Row>
      ))}
      {others.length > 0 && (
        <button
          onClick={() => setAll((v) => !v)}
          className="flex w-full items-center gap-1.5 px-4 py-2.5 text-[12px] font-medium text-fg-3 hover:text-fg"
        >
          <ChevronDown size={13} className={cx("transition-transform duration-150", all && "rotate-180")} />
          {all ? "Show fewer" : `Show all (${others.length} more)`}
        </button>
      )}

      <Row title="Another app" desc="Any MCP app, including local models: paste this into its MCP settings." error={error}>
        <Button size="sm" icon={copied ? Check : Copy} onClick={copyConfig}>
          {copied ? "Copied" : "Copy config"}
        </Button>
      </Row>
      <div className="px-4 py-3 text-[11.5px] leading-[1.45] text-fg-3">
        What the assistant reads goes to its AI provider, unless it runs a local model (for example in LM Studio).
      </div>
    </Section>
  );
}
