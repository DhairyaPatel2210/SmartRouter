import { useState } from "react";
import { ArrowDown, ArrowUp, Brain, FlaskConical, Plus, Trash2 } from "lucide-react";
import { useApp, agentName } from "../lib/store";
import { api } from "../lib/api";
import type { ExecutorEntry } from "../lib/types";
import { Badge, Button, Card, Dialog, SectionTitle, Switch } from "../components/ui";
import { TierBadge } from "../components/domain";
import { ConnectFlow } from "./CloudTab";
import { usd } from "../lib/format";

export default function PoolTab() {
  const settings = useApp((s) => s.settings)!;
  const agents = useApp((s) => s.agents);
  const saveSettings = useApp((s) => s.saveSettings);
  const toast = useApp((s) => s.toast);
  const [adding, setAdding] = useState<null | "pool" | "planner">(null);
  const [testing, setTesting] = useState<number | null>(null);
  const pool = settings.executor_pool;
  const setPool = (p: ExecutorEntry[]) => saveSettings({ executor_pool: p });
  const move = (i: number, d: number) => {
    const p = [...pool];
    const [x] = p.splice(i, 1);
    p.splice(i + d, 0, x);
    void setPool(p);
  };
  const test = async (e: ExecutorEntry, i: number) => {
    setTesting(i);
    const r = await api.smokeTest(e).finally(() => setTesting(null));
    toast(r.ok ? "info" : "warn", r.message);
  };
  const noPaid = !agents.some((a) => a.family === "paid" && a.installed && a.enabled);
  return (
    <div className="space-y-6">
      <div>
        <SectionTitle right={<Button size="sm" onClick={() => setAdding("pool")}><Plus className="h-3.5 w-3.5" /> Add cloud model</Button>}>Executor pool (cheap steps)</SectionTitle>
        <p className="text-[12.5px] text-muted mb-3">The router uses the first entry the governor allows. It falls through, and says why, when a local model isn't downloaded, the runtime is down, memory is over budget, or the Mac is hot or on battery. Add local models from the Local models tab.</p>
        <Card className="divide-y divide-line">
          {pool.length === 0 && <div className="p-4 text-[12.5px] text-muted">Empty. Without a cheap executor, every step goes to the paid agent.</div>}
          {pool.map((e, i) => (
            <div key={`${e.agent_id}:${e.model.provider_id}:${e.model.name}`} className="flex items-center gap-3 px-4 py-3">
              <span className="text-faint w-5 text-center tabular-nums">{i + 1}</span>
              <div className="flex-1 min-w-0">
                <div className="font-medium truncate">{agentName(agents, e.agent_id)} · {e.model.display_name ?? e.model.name}</div>
                <div className="text-[11.5px] text-muted flex gap-2">
                  <span className="font-mono">{e.model.provider_id}/{e.model.name}</span>
                  {e.model.tier === "local" ? <span>~{e.model.mem_needed_gb?.toFixed(1)} GB RAM</span> : e.model.price_in_per_m != null && <span>{usd(e.model.price_in_per_m, 2)}/{usd(e.model.price_out_per_m, 2)} per M</span>}
                </div>
              </div>
              <TierBadge tier={e.model.tier} />
              <Button size="sm" variant="ghost" loading={testing === i} onClick={() => test(e, i)}><FlaskConical className="h-3.5 w-3.5" /> Test</Button>
              <Button size="icon" variant="ghost" disabled={i === 0} onClick={() => move(i, -1)} title="Move up"><ArrowUp className="h-3.5 w-3.5" /></Button>
              <Button size="icon" variant="ghost" disabled={i === pool.length - 1} onClick={() => move(i, 1)} title="Move down"><ArrowDown className="h-3.5 w-3.5" /></Button>
              <Switch checked={e.enabled} onChange={(v) => setPool(pool.map((x, j) => (j === i ? { ...x, enabled: v } : x)))} />
              <Button size="icon" variant="ghost" className="hover:text-bad" onClick={() => setPool(pool.filter((_, j) => j !== i))} title="Remove"><Trash2 className="h-3.5 w-3.5" /></Button>
            </div>
          ))}
        </Card>
      </div>

      <div>
        <SectionTitle>Budget planning</SectionTitle>
        <Card className="p-4 space-y-3">
          <div className="flex items-start gap-3">
            <Brain className="h-5 w-5 text-cloud mt-0.5" />
            <div className="flex-1">
              <div className="font-medium flex items-center gap-2">Plan and review with a strong cheap-cloud model {noPaid && <Badge tone="warn">recommended: no paid agent found</Badge>}</div>
              <div className="text-[12.5px] text-muted mt-0.5">For users without a paid agent, or who want the lowest cost. Execution still follows the mode; escalation goes to this model. Runs are labelled "Budget planning" and their pass rate is tracked separately.</div>
            </div>
            <Switch checked={settings.budget_planning} disabled={!settings.budget_planner} onChange={(v) => saveSettings({ budget_planning: v })} />
          </div>
          <div className="flex items-center gap-3 pl-8">
            {settings.budget_planner ? (
              <>
                <span className="text-[12.5px]">{agentName(agents, settings.budget_planner.agent_id)} · <b>{settings.budget_planner.model.display_name ?? settings.budget_planner.model.name}</b></span>
                <TierBadge tier={settings.budget_planner.model.tier} />
              </>
            ) : (
              <span className="text-[12.5px] text-muted">No planner model chosen.</span>
            )}
            <Button size="sm" className="ml-auto" onClick={() => setAdding("planner")}>{settings.budget_planner ? "Change model" : "Choose model"}</Button>
          </div>
        </Card>
      </div>

      {adding && (
        <Dialog open onClose={() => setAdding(null)} width={680} title={adding === "planner" ? "Choose a budget planner model" : "Add a cloud model to the pool"}>
          <ConnectFlowWithProviders target={adding === "planner" ? "budget_planner" : "pool_back"} onDone={() => setAdding(null)} />
        </Dialog>
      )}
    </div>
  );
}

function ConnectFlowWithProviders({ target, onDone }: { target: "pool_back" | "budget_planner"; onDone: () => void }) {
  return <ConnectFlow defaultTarget={target} onDone={onDone} />;
}
