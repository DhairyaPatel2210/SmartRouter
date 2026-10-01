import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { BookText, Check, Download, KeyRound, Trash2 } from "lucide-react";
import { useApp } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { AgentInfo, CatalogAgent } from "../lib/types";
import { Badge, Button, Card, InlineEdit, SectionTitle, Select, Switch } from "../components/ui";
import { InstallDialog } from "./InstallDialog";
import { cn } from "../lib/format";

export default function AgentsTab() {
  const agents = useApp((s) => s.agents);
  const settings = useApp((s) => s.settings)!;
  const saveSettings = useApp((s) => s.saveSettings);
  const paid = agents.filter((a) => a.family === "paid");
  const oss = agents.filter((a) => a.family === "open_source");
  const usablePaid = paid.filter((a) => a.installed && a.enabled);
  return (
    <div className="space-y-6">
      <Card className="p-4 flex items-center gap-4">
        <div className="flex-1">
          <div className="font-medium">Planner & reviewer</div>
          <div className="text-xs text-muted mt-0.5">The paid agent that writes the plan, takes high steps and reviews at the end.</div>
        </div>
        <Select value={settings.planner_agent ?? ""} onChange={(e) => saveSettings({ planner_agent: e.target.value || null })}>
          <option value="">{usablePaid.length ? `Auto (${usablePaid[0].display_name})` : "No paid agent installed"}</option>
          {usablePaid.map((a) => <option key={a.id} value={a.id}>{a.display_name}</option>)}
        </Select>
      </Card>
      <div>
        <SectionTitle>Paid coding agents</SectionTitle>
        <div className="grid grid-cols-2 gap-3">{paid.map((a) => <AgentCard key={a.id} a={a} />)}</div>
      </div>
      <div>
        <SectionTitle>Open-source coding agents</SectionTitle>
        <div className="grid grid-cols-2 gap-3">{oss.map((a) => <AgentCard key={a.id} a={a} />)}</div>
        <p className="text-[11.5px] text-faint mt-3">Evaluating next: Qwen Code, Crush, Cline CLI, Kilo CLI (added once they have a stable headless mode and reliable local-model tool calling).</p>
      </div>
    </div>
  );
}

function AgentCard({ a }: { a: AgentInfo }) {
  const catalog = useApp((s) => s.catalog);
  const os = useApp((s) => s.os);
  const toast = useApp((s) => s.toast);
  const refreshAgents = useApp((s) => s.refreshAgents);
  const [dialog, setDialog] = useState<"install" | "uninstall" | null>(null);
  const cat: CatalogAgent | undefined = catalog.agents.find((c) => c.id === a.id);
  const installOpts = cat?.install[os] ?? [];
  const isDemo = a.id.startsWith("fake");
  const auth = a.auth;
  return (
    <Card className={cn("p-4 flex flex-col gap-2", !a.enabled && "opacity-60")}>
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <InlineEdit value={a.display_name} className="font-semibold text-[13.5px]" onSave={async (v) => { await api.agentSet(a.id, v); void refreshAgents(); }} />
          <div className="text-[12px] text-muted mt-0.5 leading-snug">{a.tagline || (isDemo ? "Scripted demo agent for trying the app without real agents." : "")}</div>
        </div>
        {a.installed && <Switch checked={a.enabled} label="Enabled" onChange={async (v) => { await api.agentSet(a.id, null, v); void refreshAgents(); }} />}
      </div>
      <div className="flex items-center gap-1.5 flex-wrap">
        {a.installed ? <Badge tone="ok"><Check className="h-3 w-3" /> {a.version ? `v${a.version}` : "installed"}</Badge> : <Badge>not installed</Badge>}
        {a.installed && auth?.state === "ok" && a.family === "paid" && <Badge tone="ok">signed in</Badge>}
        {a.installed && auth?.state === "missing" && <Badge tone="warn" title={auth.hint}>needs login</Badge>}
        {a.family === "open_source" && <Badge tone="local" title={a.providers.join(", ")}>{a.providers.some((p) => p === "ollama") ? "local + cloud models" : "cloud models"}</Badge>}
        {a.known_good_version && a.version && a.version !== a.known_good_version && <Badge title={`Tested with ${a.known_good_version}`}>tested {a.known_good_version}</Badge>}
      </div>
      <div className="flex items-center gap-2 mt-auto pt-1">
        {!a.installed && installOpts.length > 0 && (
          <Button size="sm" variant="primary" onClick={() => setDialog("install")}><Download className="h-3.5 w-3.5" /> Install</Button>
        )}
        {a.installed && a.login_command && auth?.state !== "ok" && (
          <Button
            size="sm"
            onClick={async () => {
              try {
                await api.openLogin(a.id);
                toast("info", "Finish signing in in Terminal, then press Re-detect.");
              } catch (e) {
                toast("error", errorText(e));
              }
            }}
          >
            <KeyRound className="h-3.5 w-3.5" /> Sign in
          </Button>
        )}
        {a.docs && <Button size="sm" variant="ghost" onClick={() => openUrl(a.docs)}><BookText className="h-3.5 w-3.5" /> Docs</Button>}
        {a.installed && cat?.uninstall[os] && (
          <Button size="icon" variant="ghost" className="ml-auto hover:text-bad" title="Uninstall" onClick={() => setDialog("uninstall")}><Trash2 className="h-3.5 w-3.5" /></Button>
        )}
      </div>
      {a.path && <div className="text-[10.5px] text-faint font-mono truncate selectable" title={a.path}>{a.path}</div>}
      {dialog === "install" && <InstallDialog id={a.id} kind="agent" title={`Install ${a.display_name}`} options={installOpts} onClose={() => setDialog(null)} />}
      {dialog === "uninstall" && cat && <InstallDialog id={a.id} kind="uninstall" title={`Uninstall ${a.display_name}`} options={[{ label: "Uninstall command", command: cat.uninstall[os] }]} onClose={() => setDialog(null)} />}
    </Card>
  );
}
