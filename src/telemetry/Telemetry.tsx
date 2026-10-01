import { useEffect, useMemo, useRef, useState } from "react";
import { Activity, Battery, Cpu, Flame, Gauge, HelpCircle, MemoryStick, OctagonX, Zap } from "lucide-react";
import { useApp, modeName } from "../lib/store";
import { api } from "../lib/api";
import type { OutcomeStat, RunRow, Snapshot, WhySlow } from "../lib/types";
import { buildSuggestions } from "../lib/insights";
import { Badge, Button, Card, Dialog, Empty, SectionTitle, Segmented, Select, Spinner, Tabs } from "../components/ui";
import { RunStatusBadge, TierBar } from "../components/domain";
import { ScreenHeader } from "../components/Shell";
import { ago, cn, duration, gb, mb, usd } from "../lib/format";

type Tab = "resources" | "history" | "insights";

export default function Telemetry() {
  const [tab, setTab] = useState<Tab>("resources");
  return (
    <div className="flex flex-col h-full">
      <ScreenHeader title="Telemetry" subtitle="Local observability only: nothing here leaves your Mac." />
      <div className="px-6 shrink-0">
        <Tabs value={tab} onChange={setTab} tabs={[{ value: "resources", label: "Resources" }, { value: "history", label: "Run history" }, { value: "insights", label: "Routing insights" }]} />
      </div>
      <div className="flex-1 overflow-auto">
        <div className="max-w-[1000px] mx-auto px-6 py-5">
          {tab === "resources" && <Resources />}
          {tab === "history" && <History />}
          {tab === "insights" && <Insights />}
        </div>
      </div>
    </div>
  );
}

const WINDOW = 150; // 5 minutes at 2 s

function Resources() {
  const snap = useApp((s) => s.snapshot);
  const toast = useApp((s) => s.toast);
  const series = useRef<Snapshot[]>([]);
  const [, force] = useState(0);
  const [why, setWhy] = useState<WhySlow | null>(null);
  const [whyLoading, setWhyLoading] = useState(false);

  // Sampling runs only while this view is open (or a run is active).
  useEffect(() => {
    api.resourcesView(true).then((s) => useApp.setState({ snapshot: s }), () => {});
    return () => {
      void api.resourcesView(false);
    };
  }, []);
  useEffect(() => {
    if (!snap) return;
    const last = series.current[series.current.length - 1];
    if (!last || last.ts !== snap.ts) {
      series.current = [...series.current.slice(-(WINDOW - 1)), snap];
      force((x) => x + 1);
    }
  }, [snap]);

  if (!snap) return <Spinner />;
  const s = snap.sys;
  const agentsMb = snap.procs.filter((p) => p.owner !== "ollama").reduce((a, p) => a + p.rss_mb, 0);
  const modelsMb = Math.max(snap.models.reduce((a, m) => a + m.mem_mb, 0), snap.procs.filter((p) => p.owner === "ollama").reduce((a, p) => a + p.rss_mb, 0));
  return (
    <div className="space-y-5">
      <div className="grid grid-cols-4 gap-3">
        <Metric icon={<MemoryStick className="h-4 w-4" />} label="Memory" value={`${gb(s.used_gb)} / ${gb(s.total_gb)}`} sub={`${gb(s.free_gb)} free`} />
        <Metric icon={<Gauge className="h-4 w-4" />} label="Pressure" value={<span className={cn(s.pressure === "critical" ? "text-bad" : s.pressure === "warning" ? "text-warn" : "text-ok")}>{s.pressure ?? "—"}</span>} sub="macOS memory pressure" />
        <Metric icon={<Flame className="h-4 w-4" />} label="Thermal" value={<span className={cn(s.thermal === "serious" || s.thermal === "critical" ? "text-warn" : "")}>{s.thermal ?? "—"}</span>} sub="thermal state" />
        <Metric icon={<Battery className="h-4 w-4" />} label="Power" value={s.power === "battery" ? "Battery" : s.power === "ac" ? "Plugged in" : "—"} sub={s.low_power ? "Low Power Mode on" : snap.sampling ? "sampling every 2 s" : "idle"} />
      </div>

      <Card className="p-4">
        <SectionTitle right={
          <div className="flex items-center gap-3 text-[11px] text-muted">
            <Legend color="bg-premium" label="Agents" />
            <Legend color="bg-local" label="Local models" />
            <Legend color="bg-line-strong" label="Other apps" />
          </div>
        }>Memory over the last 5 minutes</SectionTitle>
        <StackedArea data={series.current} />
      </Card>

      <div className="grid grid-cols-2 gap-4">
        <Card className="p-4">
          <SectionTitle right={<span className="text-xs text-muted">{mb(agentsMb)}</span>}>Agents & tools</SectionTitle>
          {snap.procs.length === 0 && <div className="text-[12.5px] text-muted">Nothing running.</div>}
          {snap.procs.map((p) => <UsageRow key={p.pid} label={p.label} sub={`${p.processes} process${p.processes === 1 ? "" : "es"} · pid ${p.pid}`} mb={p.rss_mb} cpu={p.cpu_pct} total={s.total_gb * 1024} />)}
          {snap.app && <UsageRow label="This app" sub="core + webview" mb={snap.app.rss_mb} cpu={snap.app.cpu_pct} total={s.total_gb * 1024} muted />}
        </Card>
        <Card className="p-4">
          <SectionTitle right={<span className="text-xs text-muted">{mb(modelsMb)}</span>}>Local models</SectionTitle>
          {snap.models.length === 0 && <div className="text-[12.5px] text-muted">No models loaded.</div>}
          {snap.models.map((m) => (
            <div key={m.name} className="flex items-center gap-3 py-2">
              <div className="flex-1 min-w-0">
                <div className="font-medium truncate">{m.name} {m.loaded_by_app && <Badge tone="accent">loaded by app</Badge>}</div>
                <div className="text-[11.5px] text-muted">{mb(m.mem_mb)} · unloads {m.expires_at ? untilText(m.expires_at) : "when idle"}</div>
              </div>
              <Button size="sm" onClick={async () => { const g = await api.unloadModel(m.name); toast("info", `Unloaded ${m.name} (${g.toFixed(1)} GB freed).`); }}>Unload</Button>
            </div>
          ))}
        </Card>
      </div>

      <div className="flex items-center gap-2">
        <Button onClick={async () => { setWhyLoading(true); setWhy(await api.whySlow().finally(() => setWhyLoading(false))); }} loading={whyLoading}>
          <HelpCircle className="h-4 w-4" /> Why is my Mac slow?
        </Button>
        <Button variant="outline" className="text-bad" onClick={async () => toast("warn", await api.stopEverything())}>
          <OctagonX className="h-4 w-4" /> Stop everything
        </Button>
      </div>

      {why && (
        <Dialog open onClose={() => setWhy(null)} title="Why is my Mac slow?" width={560} footer={<Button onClick={() => setWhy(null)}>Close</Button>}>
          <div className="space-y-4 text-[12.5px]">
            <ul className="space-y-1">{why.advice.map((a) => <li key={a} className="flex gap-2"><Zap className="h-3.5 w-3.5 text-accent mt-0.5" />{a}</li>)}</ul>
            <div>
              <div className="font-semibold mb-1">What this app controls</div>
              {why.snapshot.procs.length + why.snapshot.models.length === 0 && <div className="text-muted">Nothing heavy right now.</div>}
              {why.snapshot.procs.map((p) => <div key={p.pid} className="flex justify-between py-0.5"><span>{p.label}</span><span className="text-muted">{mb(p.rss_mb)} · {p.cpu_pct.toFixed(0)}% CPU</span></div>)}
              {why.snapshot.models.map((m) => (
                <div key={m.name} className="flex items-center justify-between py-0.5">
                  <span>{m.name}</span>
                  <span className="flex items-center gap-2 text-muted">{mb(m.mem_mb)} <Button size="sm" variant="ghost" onClick={() => api.unloadModel(m.name)}>Unload</Button></span>
                </div>
              ))}
            </div>
            <div>
              <div className="font-semibold mb-1">Heavy apps it doesn't control</div>
              {why.others.map((o) => <div key={o.pid} className="flex justify-between py-0.5"><span className="truncate">{o.name}</span><span className="text-muted">{mb(o.rss_mb)}</span></div>)}
            </div>
          </div>
        </Dialog>
      )}
    </div>
  );
}

function untilText(iso: string): string {
  const t = Date.parse(iso);
  if (!t) return "when idle";
  const ms = t - Date.now();
  if (ms <= 0) return "now";
  if (ms > 365 * 86400_000) return "never (kept loaded)";
  return `in ${duration(ms)}`;
}

function Legend({ color, label }: { color: string; label: string }) {
  return <span className="flex items-center gap-1"><span className={cn("h-2 w-2 rounded-sm", color)} />{label}</span>;
}

function Metric({ icon, label, value, sub }: { icon: React.ReactNode; label: string; value: React.ReactNode; sub?: string }) {
  return (
    <Card className="p-3.5">
      <div className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-faint">{icon}{label}</div>
      <div className="text-[15px] font-semibold mt-1 capitalize">{value}</div>
      {sub && <div className="text-[11.5px] text-muted">{sub}</div>}
    </Card>
  );
}

function UsageRow({ label, sub, mb: m, cpu, total, muted }: { label: string; sub: string; mb: number; cpu: number; total: number; muted?: boolean }) {
  return (
    <div className={cn("py-2", muted && "opacity-70")}>
      <div className="flex items-center gap-2">
        <span className="flex-1 truncate font-medium">{label}</span>
        <span className="text-xs text-muted flex items-center gap-1"><Cpu className="h-3 w-3" />{cpu.toFixed(0)}%</span>
        <span className="text-xs w-16 text-right">{mb(m)}</span>
      </div>
      <div className="text-[11px] text-faint">{sub}</div>
      <div className="h-1 mt-1 rounded-full bg-panel-2 overflow-hidden"><div className="h-full bg-premium" style={{ width: `${Math.min(100, (m / total) * 100 * 4)}%` }} /></div>
    </div>
  );
}

/** Lightweight stacked area chart (no chart library; see docs/DECISIONS.md). */
function StackedArea({ data }: { data: Snapshot[] }) {
  const W = 900;
  const H = 140;
  if (data.length < 2) return <div className="h-[140px] flex items-center justify-center text-[12px] text-faint">Collecting samples…</div>;
  const total = data[0].sys.total_gb * 1024;
  const pts = data.map((d) => {
    const agents = d.procs.filter((p) => p.owner !== "ollama").reduce((a, p) => a + p.rss_mb, 0);
    const models = Math.max(d.models.reduce((a, m) => a + m.mem_mb, 0), d.procs.filter((p) => p.owner === "ollama").reduce((a, p) => a + p.rss_mb, 0));
    const used = d.sys.used_gb * 1024;
    return { agents, models, other: Math.max(0, used - agents - models) };
  });
  const x = (i: number) => (i / (WINDOW - 1)) * W + (W - ((pts.length - 1) / (WINDOW - 1)) * W);
  const y = (v: number) => H - (v / total) * H;
  const area = (lo: (p: (typeof pts)[0]) => number, hi: (p: (typeof pts)[0]) => number) =>
    `M ${pts.map((p, i) => `${x(i)},${y(hi(p))}`).join(" L ")} L ${pts.map((_, i) => `${x(pts.length - 1 - i)},${y(lo(pts[pts.length - 1 - i]))}`).join(" L ")} Z`;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="w-full h-[140px]" preserveAspectRatio="none" role="img" aria-label="Memory use over time">
      {[0.25, 0.5, 0.75].map((f) => <line key={f} x1={0} x2={W} y1={H * f} y2={H * f} stroke="var(--line)" strokeDasharray="3 4" />)}
      <path d={area((p) => p.agents + p.models, (p) => p.agents + p.models + p.other)} fill="var(--line-strong)" opacity={0.6} />
      <path d={area((p) => p.agents, (p) => p.agents + p.models)} fill="var(--local)" opacity={0.7} />
      <path d={area(() => 0, (p) => p.agents)} fill="var(--premium)" opacity={0.8} />
    </svg>
  );
}

// ------------------------------------------------------------------ history

function History() {
  const recent = useApp((s) => s.recent);
  const settings = useApp((s) => s.settings);
  const viewRun = useApp((s) => s.viewRun);
  const live = useApp((s) => s.runs);
  const [ws, setWs] = useState<string>("");
  const [range, setRange] = useState<"1" | "7" | "30" | "all">("30");
  const [runs, setRuns] = useState<RunRow[] | null>(null);
  useEffect(() => {
    const since = range === "all" ? null : Date.now() - Number(range) * 86400_000;
    api.listRuns(ws || null, since, 500).then(setRuns, () => setRuns([]));
  }, [ws, range, live]);
  const totals = useMemo(() => {
    const r = runs ?? [];
    return { cost: r.reduce((a, x) => a + x.est_cost_usd, 0), saved: r.reduce((a, x) => a + (x.summary.saved_usd ?? 0), 0), n: r.length, ok: r.filter((x) => x.status === "succeeded").length };
  }, [runs]);
  const wsName = (id: string) => recent.find((w) => w.id === id)?.display_name ?? "—";
  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <Select value={ws} onChange={(e) => setWs(e.target.value)}>
          <option value="">All workspaces</option>
          {recent.map((w) => <option key={w.id} value={w.id}>{w.display_name}</option>)}
        </Select>
        <Segmented size="sm" value={range} onChange={setRange} options={[{ value: "1", label: "Today" }, { value: "7", label: "7 days" }, { value: "30", label: "30 days" }, { value: "all", label: "All" }]} />
        <span className="ml-auto text-[12.5px] text-muted">{totals.n} runs · {totals.ok} succeeded · est. {usd(totals.cost, 2)} · saved ~{usd(totals.saved, 2)} vs all-paid</span>
      </div>
      <Card className="overflow-hidden">
        {runs === null && <div className="p-4"><Spinner /></div>}
        {runs?.length === 0 && <Empty icon={<Activity className="h-7 w-7" />} title="No runs in this range" />}
        {runs && runs.length > 0 && (
          <table className="w-full text-[12.5px]">
            <thead className="text-[11px] uppercase tracking-wider text-faint bg-panel-2">
              <tr>
                <th className="text-left font-semibold px-4 py-2">Goal</th>
                <th className="text-left font-semibold px-2">Workspace</th>
                <th className="text-left font-semibold px-2">Mode</th>
                <th className="text-left font-semibold px-2 w-28">Tiers</th>
                <th className="text-right font-semibold px-2">Cost</th>
                <th className="text-right font-semibold px-2">Peak mem</th>
                <th className="text-right font-semibold px-2">Time</th>
                <th className="text-left font-semibold px-4">Result</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-line">
              {runs.map((r) => (
                <tr key={r.id} onClick={() => viewRun(r.id)} className="hover:bg-panel-2 cursor-default">
                  <td className="px-4 py-2 max-w-[280px]"><div className="truncate font-medium">{r.goal}</div><div className="text-[11px] text-faint">{ago(r.started_at)}{r.budget_planning ? " · budget planning" : ""}</div></td>
                  <td className="px-2 text-muted truncate max-w-[120px]">{wsName(r.workspace_id)}</td>
                  <td className="px-2 text-muted">{modeName(settings, r.mode)}</td>
                  <td className="px-2"><TierBar local={r.summary.local_steps ?? 0} cloud={r.summary.cheap_cloud_steps ?? 0} premium={r.summary.premium_steps ?? 0} /><div className="text-[10.5px] text-faint mt-0.5">{r.summary.steps_passed ?? 0}/{r.summary.steps_total ?? 0} steps</div></td>
                  <td className="px-2 text-right">{usd(r.est_cost_usd, 2)}</td>
                  <td className="px-2 text-right text-muted">{r.peak_mem_mb ? mb(r.peak_mem_mb) : "—"}</td>
                  <td className="px-2 text-right text-muted">{r.ended_at ? duration(r.ended_at - r.started_at) : "—"}</td>
                  <td className="px-4"><RunStatusBadge status={r.status} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </div>
  );
}

// ------------------------------------------------------------------ insights

function Insights() {
  const workspace = useApp((s) => s.workspace);
  const [scope, setScope] = useState<"ws" | "all">(workspace ? "ws" : "all");
  const [stats, setStats] = useState<OutcomeStat[] | null>(null);
  useEffect(() => {
    api.outcomeStats(scope === "ws" ? workspace?.row.id ?? null : null).then(setStats, () => setStats([]));
  }, [scope, workspace?.row.id]);
  const suggestions = useMemo(() => buildSuggestions(stats ?? []), [stats]);
  const bp = useMemo(() => {
    const s = stats ?? [];
    const rate = (b: boolean) => {
      const x = s.filter((r) => r.budget_planning === b);
      const t = x.reduce((a, r) => a + r.total, 0);
      return t ? { rate: x.reduce((a, r) => a + r.first_try_passes, 0) / t, n: t } : null;
    };
    return { on: rate(true), off: rate(false) };
  }, [stats]);
  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <Segmented size="sm" value={scope} onChange={setScope} options={[{ value: "ws", label: "This workspace" }, { value: "all", label: "All workspaces" }]} />
        <span className="text-[12px] text-muted">First-try pass rate by step class, tier and model. The router uses this to suggest reassignments.</span>
      </div>
      {suggestions.length > 0 && (
        <Card className="p-4 space-y-1.5">
          <SectionTitle>Suggestions</SectionTitle>
          {suggestions.map((s) => <div key={s} className="text-[12.5px] flex gap-2"><Zap className="h-3.5 w-3.5 text-accent mt-0.5 shrink-0" />{s}</div>)}
        </Card>
      )}
      {(bp.on || bp.off) && (
        <div className="grid grid-cols-2 gap-3">
          <Metric icon={<Activity className="h-4 w-4" />} label="Paid planner" value={bp.off ? `${Math.round(bp.off.rate * 100)}%` : "—"} sub={bp.off ? `${bp.off.n} steps passed first try` : "no runs"} />
          <Metric icon={<Activity className="h-4 w-4" />} label="Budget planning" value={bp.on ? `${Math.round(bp.on.rate * 100)}%` : "—"} sub={bp.on ? `${bp.on.n} steps passed first try` : "no runs"} />
        </div>
      )}
      <Card className="overflow-hidden">
        {stats === null && <div className="p-4"><Spinner /></div>}
        {stats?.length === 0 && <Empty title="No finished steps yet" />}
        {stats && stats.length > 0 && (
          <table className="w-full text-[12.5px]">
            <thead className="text-[11px] uppercase tracking-wider text-faint bg-panel-2">
              <tr><th className="text-left px-4 py-2">Class</th><th className="text-left px-2">Tier</th><th className="text-left px-2">Model / agent</th><th className="text-right px-2">Steps</th><th className="text-left px-4 w-48">First-try pass</th></tr>
            </thead>
            <tbody className="divide-y divide-line">
              {stats.map((s, i) => {
                const r = s.total ? s.first_try_passes / s.total : 0;
                return (
                  <tr key={i}>
                    <td className="px-4 py-2">{s.class}</td>
                    <td className="px-2">{s.tier}</td>
                    <td className="px-2 font-mono text-[11.5px] truncate max-w-[260px]">{s.model}{s.budget_planning ? " (budget planning)" : ""}</td>
                    <td className="px-2 text-right">{s.total}</td>
                    <td className="px-4"><div className="flex items-center gap-2"><div className="flex-1 h-1.5 rounded-full bg-panel-2"><div className={cn("h-full rounded-full", r >= 0.7 ? "bg-ok" : r >= 0.4 ? "bg-warn" : "bg-bad")} style={{ width: `${r * 100}%` }} /></div><span className="w-9 text-right">{Math.round(r * 100)}%</span></div></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </Card>
    </div>
  );
}

