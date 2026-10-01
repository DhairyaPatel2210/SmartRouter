import { useEffect, useRef, useState } from "react";
import { Terminal } from "lucide-react";
import { useApp } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { InstallOption } from "../lib/types";
import { Button, Dialog, Spinner } from "../components/ui";
import { cn } from "../lib/format";

/** Shows every command before it runs, then streams its output. */
export function InstallDialog({
  id,
  title,
  options,
  kind,
  onClose,
}: {
  id: string;
  title: string;
  options: InstallOption[];
  kind: "agent" | "runtime" | "uninstall";
  onClose: () => void;
}) {
  const toast = useApp((s) => s.toast);
  const job = useApp((s) => s.installs[id]);
  const [choice, setChoice] = useState(0);
  const [started, setStarted] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);
  useEffect(() => {
    logRef.current?.scrollTo(0, logRef.current.scrollHeight);
  }, [job?.lines.length]);
  const run = async () => {
    useApp.setState((s) => ({ installs: { ...s.installs, [id]: { lines: [], done: null } } }));
    setStarted(true);
    try {
      if (kind === "agent") await api.installAgent(id, choice);
      else if (kind === "runtime") await api.installRuntime(id, choice);
      else await api.uninstallAgent(id);
    } catch (e) {
      toast("error", errorText(e));
      setStarted(false);
    }
  };
  const running = started && job?.done == null;
  return (
    <Dialog
      open
      onClose={() => !running && onClose()}
      dismissable={!running}
      title={title}
      width={620}
      footer={
        <>
          {running ? (
            <Button variant="ghost" onClick={() => api.cancelJob(id)}>Cancel</Button>
          ) : (
            <Button variant="ghost" onClick={onClose}>{job?.done != null ? "Close" : "Cancel"}</Button>
          )}
          {!started && (
            <Button variant={kind === "uninstall" ? "danger" : "primary"} onClick={run}>
              <Terminal className="h-3.5 w-3.5" /> Run this command
            </Button>
          )}
        </>
      }
    >
      {!started && (
        <div className="space-y-2">
          <p className="text-[12.5px] text-muted">The app runs exactly this in your login shell. Review it first.</p>
          {options.map((o, i) => (
            <label key={i} className={cn("block rounded-lg border p-3 cursor-default", choice === i ? "border-accent bg-accent-soft/30" : "border-line")}>
              <div className="flex items-center gap-2">
                {options.length > 1 && <input type="radio" checked={choice === i} onChange={() => setChoice(i)} />}
                <span className="font-medium text-[12.5px]">{o.label}</span>
              </div>
              <code className="block mt-1.5 text-[12px] font-mono bg-panel-2 rounded px-2 py-1.5 selectable break-all">{o.command}</code>
            </label>
          ))}
        </div>
      )}
      {started && (
        <div>
          <div className="flex items-center gap-2 mb-2 text-[12.5px]">
            {running ? <Spinner className="h-3.5 w-3.5" /> : null}
            <span className={cn(job?.done === true && "text-ok", job?.done === false && "text-bad")}>
              {running ? "Running…" : job?.done ? "Finished" : "Failed"}
            </span>
          </div>
          <pre ref={logRef} className="h-64 overflow-auto rounded-lg bg-[#0d0d11] text-[#d4d4dc] p-3 text-[11.5px] font-mono whitespace-pre-wrap selectable">
            {(job?.lines ?? []).join("\n")}
          </pre>
        </div>
      )}
    </Dialog>
  );
}
