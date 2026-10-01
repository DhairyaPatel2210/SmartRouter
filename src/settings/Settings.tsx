import { useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Copy, Plus, RotateCcw, Terminal, Trash2, X } from "lucide-react";
import { useApp } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { DirtyStrategy, ModeDef, RunBudget, Settings as S, WorkspaceRow } from "../lib/types";
import { Badge, Button, Card, Field, InlineEdit, Input, Segmented, Select, Switch } from "../components/ui";
import { ScreenHeader } from "../components/Shell";
import { cn, shortPath } from "../lib/format";

type Section = "general" | "routing" | "approvals" | "resources" | "workspace" | "prices" | "privacy";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "general", label: "General" },
  { id: "routing", label: "Routing & modes" },
  { id: "approvals", label: "Approvals & budgets" },
  { id: "resources", label: "Resources" },
  { id: "workspace", label: "This workspace" },
  { id: "prices", label: "Prices" },
  { id: "privacy", label: "Privacy" },
];

export default function Settings() {
  const [sec, setSec] = useState<Section>("general");
  const settings = useApp((s) => s.settings);
  if (!settings) return null;
  return (
    <div className="flex flex-col h-full">
      <ScreenHeader title="Settings" />
      <div className="flex-1 flex min-h-0">
        <nav className="w-48 shrink-0 border-r border-line p-3 space-y-0.5">
          {SECTIONS.map((s) => (
            <button key={s.id} onClick={() => setSec(s.id)} className={cn("w-full text-left h-8 px-3 rounded-lg text-[13px]", sec === s.id ? "bg-panel-2 text-fg font-medium" : "text-muted hover:text-fg")}>{s.label}</button>
          ))}
        </nav>
        <div className="flex-1 overflow-auto">
          <div className="max-w-[720px] px-8 py-6">
            {sec === "general" && <General />}
            {sec === "routing" && <Routing />}
            {sec === "approvals" && <Approvals />}
            {sec === "resources" && <Resources />}
            {sec === "workspace" && <WorkspaceSettings />}
            {sec === "prices" && <Prices />}
            {sec === "privacy" && <Privacy />}
          </div>
        </div>
      </div>
    </div>
  );
}

function useSettings(): [S, (p: Partial<S>) => Promise<void>] {
  const s = useApp((st) => st.settings)!;
  const save = useApp((st) => st.saveSettings);
  return [s, save];
}

function NumberInput({ value, onCommit, placeholder, min, max, step, className }: { value: number | null | undefined; onCommit: (v: number | null) => void; placeholder?: string; min?: number; max?: number; step?: number; className?: string }) {
  const [v, setV] = useState(value == null ? "" : String(value));
  useEffect(() => setV(value == null ? "" : String(value)), [value]);
  return (
    <Input
      type="number"
      className={cn("w-24 text-right", className)}
      value={v}
      min={min}
      max={max}
      step={step}
      placeholder={placeholder}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => onCommit(v === "" ? null : Number(v))}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
    />
  );
}

function General() {
  const [s, save] = useSettings();
  const brand = useApp((st) => st.brand);
  const dataDir = useApp((st) => st.dataDir);
  const version = useApp((st) => st.version);
  const toast = useApp((st) => st.toast);
  return (
    <Card className="px-5">
      <Field label="Appearance">
        <Segmented value={s.theme} onChange={(theme) => save({ theme })} options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} />
      </Field>
      <Field label="Menu bar item" hint="Shows what's running and a Stop everything button. With it on, closing the window keeps the app in the menu bar. Applies after restart.">
        <Switch checked={s.menu_bar} onChange={(menu_bar) => save({ menu_bar })} />
      </Field>
      <Field label="Notifications" hint="Governor actions and approvals notify you when the window is hidden.">
        <Switch checked={s.notifications} onChange={(notifications) => save({ notifications })} />
      </Field>
      <Field label={`Shell command \`${brand.cliName}\``} hint={`Open a folder from the terminal: ${brand.cliName} ~/code/project`}>
        <Button size="sm" onClick={async () => { try { toast("info", `Installed ${await api.installCliCommand()}. Make sure ~/.local/bin is on your PATH.`); } catch (e) { toast("error", errorText(e)); } }}><Terminal className="h-3.5 w-3.5" /> Install</Button>
      </Field>
      <Field label="Product name" hint="Set in brand.json (productName, shortName, identifiers) and applied at build time by scripts/apply-brand. Item names are renamed inline anywhere they appear.">
        <span className="text-[12.5px] text-muted">{brand.productName}</span>
      </Field>
      <Field label="Data folder" hint="Database, logs, snapshots and the global Library.">
        <Button size="sm" variant="ghost" onClick={() => revealItemInDir(dataDir)}>{shortPath(dataDir)}</Button>
      </Field>
      <Field label="Onboarding">
        <Button size="sm" variant="ghost" onClick={() => useApp.setState({ onboardingOpen: true })}><RotateCcw className="h-3.5 w-3.5" /> Run again</Button>
      </Field>
      <Field label="Version"><span className="text-[12.5px] text-muted">{version}</span></Field>
    </Card>
  );
}

function Routing() {
  const [s, save] = useSettings();
  const [edit, setEdit] = useState<string | null>(null);
  const setMode = (m: ModeDef) => save({ modes: s.modes.map((x) => (x.id === m.id ? m : x)) });
  const duplicate = (m: ModeDef) => {
    const id = `${m.id}-copy-${Math.random().toString(36).slice(2, 6)}`;
    void save({ modes: [...s.modes, { ...m, id, display_name: `${m.display_name} copy`, builtin: false }] });
    setEdit(id);
  };
  return (
    <div className="space-y-4">
      <Card className="px-5">
        <Field label="Default mode" hint="Each workspace can override it; you can switch per run and mid-run.">
          <Select value={s.default_mode} onChange={(e) => save({ default_mode: e.target.value })}>
            {s.modes.map((m) => <option key={m.id} value={m.id}>{m.display_name}</option>)}
          </Select>
        </Field>
      </Card>
      <div className="text-[11px] font-semibold uppercase tracking-wider text-faint">Modes</div>
      {s.modes.map((m) => (
        <Card key={m.id} className="p-4">
          <div className="flex items-center gap-2">
            <InlineEdit value={m.display_name} className="font-semibold" onSave={(v) => setMode({ ...m, display_name: v })} />
            {m.builtin ? <Badge>built-in</Badge> : <Badge tone="accent">custom</Badge>}
            <span className="ml-auto flex gap-1">
              <Button size="sm" variant="ghost" onClick={() => duplicate(m)}><Copy className="h-3.5 w-3.5" /> Duplicate</Button>
              {!m.builtin && <Button size="sm" variant="ghost" onClick={() => setEdit(edit === m.id ? null : m.id)}>{edit === m.id ? "Done" : "Edit"}</Button>}
              {!m.builtin && <Button size="icon" variant="ghost" className="hover:text-bad" onClick={() => save({ modes: s.modes.filter((x) => x.id !== m.id), default_mode: s.default_mode === m.id ? "balanced" : s.default_mode })}><Trash2 className="h-3.5 w-3.5" /></Button>}
            </span>
          </div>
          <div className="text-[12.5px] text-muted mt-1">{m.description}</div>
          <ModeTable m={m} editable={edit === m.id} onChange={setMode} />
        </Card>
      ))}
    </div>
  );
}

function ModeTable({ m, editable, onChange }: { m: ModeDef; editable: boolean; onChange: (m: ModeDef) => void }) {
  const exec = (k: "high" | "low" | "trivial") =>
    editable ? (
      <Select value={m[k]} onChange={(e) => onChange({ ...m, [k]: e.target.value as ModeDef["high"] })}><option value="paid">Paid</option><option value="cheap">Cheap</option></Select>
    ) : (
      <Badge tone={m[k] === "paid" ? "premium" : "local"}>{m[k] === "paid" ? "Paid" : "Cheap"}</Badge>
    );
  return (
    <div className="grid grid-cols-4 gap-3 mt-3 text-[12px]">
      <Cell label="High steps">{exec("high")}</Cell>
      <Cell label="Low steps">{exec("low")}</Cell>
      <Cell label="Trivial steps">{exec("trivial")}</Cell>
      <Cell label="Unclear steps">
        {editable ? <Select value={m.unclear_as} onChange={(e) => onChange({ ...m, unclear_as: e.target.value as ModeDef["unclear_as"] })}><option value="high">High</option><option value="low">Low</option></Select> : <span className="capitalize">{m.unclear_as}</span>}
      </Cell>
      <Cell label="Final review">{editable ? <Switch checked={m.review} onChange={(review) => onChange({ ...m, review })} /> : m.review ? "Paid, once" : "None"}</Cell>
      <Cell label="Escalation to paid">
        {editable ? <Select value={m.escalation} onChange={(e) => onChange({ ...m, escalation: e.target.value as ModeDef["escalation"] })}><option value="auto">Automatic</option><option value="approval">Ask first</option><option value="never">Never</option></Select> : m.escalation === "auto" ? "Automatic" : m.escalation === "approval" ? "Ask first" : "Never"}
      </Cell>
      <Cell label="Attempts per executor">{editable ? <NumberInput value={m.attempts_per_executor} min={1} max={5} onCommit={(v) => onChange({ ...m, attempts_per_executor: Math.max(1, v ?? 2) })} className="w-16" /> : m.attempts_per_executor}</Cell>
      <Cell label="Plan length">{editable ? <Switch checked={m.short_plan} onChange={(short_plan) => onChange({ ...m, short_plan })} /> : m.short_plan ? "Short" : "Normal"}</Cell>
    </div>
  );
}

function Cell({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-[10.5px] uppercase tracking-wider text-faint mb-1">{label}</div>
      <div>{children}</div>
    </div>
  );
}

function BudgetFields({ b, onChange }: { b: RunBudget; onChange: (b: RunBudget) => void }) {
  return (
    <>
      <Field label="Max estimated spend per run" hint="Pause and ask when reached (paid + cloud, estimated).">
        <span className="text-muted">$</span><NumberInput value={b.max_cost_usd} step={0.5} min={0} placeholder="none" onCommit={(v) => onChange({ ...b, max_cost_usd: v })} />
      </Field>
      <Field label="Max steps per run"><NumberInput value={b.max_steps} min={1} placeholder="none" onCommit={(v) => onChange({ ...b, max_steps: v })} /></Field>
      <Field label="Max paid tokens per run"><NumberInput value={b.max_paid_tokens} min={0} step={10000} placeholder="none" onCommit={(v) => onChange({ ...b, max_paid_tokens: v })} /></Field>
    </>
  );
}

function Approvals() {
  const [s, save] = useSettings();
  return (
    <div className="space-y-4">
      <Card className="px-5">
        <Field label="Approve before paid calls" hint="Pause before every planning, review or step that uses the paid agent."><Switch checked={s.approve_before_paid} onChange={(approve_before_paid) => save({ approve_before_paid })} /></Field>
        <Field label="Approve before cheap cloud calls" hint="Pause before every step on a cloud model."><Switch checked={s.approve_before_cloud} onChange={(approve_before_cloud) => save({ approve_before_cloud })} /></Field>
        <Field label="Step timeout" hint="Agents are stopped (whole process tree) after this long."><NumberInput value={s.step_timeout_minutes} min={1} max={120} onCommit={(v) => save({ step_timeout_minutes: v ?? 15 })} /><span className="text-muted text-xs">min</span></Field>
        <Field label="Runs at once" hint="Across workspaces. On ≤16 GB Macs only one run uses a local model at a time; cloud runs can run alongside."><NumberInput value={s.global_concurrency} min={1} max={6} onCommit={(v) => save({ global_concurrency: Math.max(1, v ?? 2) })} /></Field>
      </Card>
      <div className="text-[11px] font-semibold uppercase tracking-wider text-faint">Default budget cap</div>
      <Card className="px-5"><BudgetFields b={s.run_budget} onChange={(run_budget) => save({ run_budget })} /></Card>
    </div>
  );
}

function Resources() {
  const [s, save] = useSettings();
  const [auto, setAuto] = useState(s.memory_budget_pct == null);
  return (
    <Card className="px-5">
      <Field label="Local-model memory budget" hint="Local models may use at most this share of unified memory. Auto = 60% on ≤16 GB Macs, 70% above.">
        <Switch checked={auto} onChange={(v) => { setAuto(v); void save({ memory_budget_pct: v ? null : 60 }); }} label="Auto" />
        <span className="text-xs text-muted">Auto</span>
        {!auto && (
          <>
            <input type="range" min={30} max={85} step={5} value={s.memory_budget_pct ?? 60} onChange={(e) => save({ memory_budget_pct: Number(e.target.value) })} className="w-32 accent-[var(--accent)]" />
            <span className="w-9 text-right text-[12.5px]">{s.memory_budget_pct}%</span>
          </>
        )}
      </Field>
      <Field label="Unload idle models after" hint="Also unloaded when a run ends and nothing is queued."><NumberInput value={s.idle_unload_minutes} min={1} max={120} onCommit={(v) => save({ idle_unload_minutes: v ?? 5 })} /><span className="text-muted text-xs">min</span></Field>
      <Field label="Prefer cloud on battery" hint="New cheap steps go to a cloud model (if the pool has one) while unplugged."><Switch checked={s.prefer_cloud_on_battery} onChange={(prefer_cloud_on_battery) => save({ prefer_cloud_on_battery })} /></Field>
      <Field label="Prefer cloud when hot" hint="At serious thermal state, new cheap steps go to cloud."><Switch checked={s.prefer_cloud_when_hot} onChange={(prefer_cloud_when_hot) => save({ prefer_cloud_when_hot })} /></Field>
      <Field label="Keep the Mac responsive" hint="Agents, checks and the model server run at lower priority so your editor and browser stay smooth."><Switch checked={s.lower_priority} onChange={(lower_priority) => save({ lower_priority })} /></Field>
      <Field label="Let the app start Ollama" hint="Started only while needed and stopped afterwards. An Ollama you started yourself is never stopped."><Switch checked={s.manage_ollama} onChange={(manage_ollama) => save({ manage_ollama })} /></Field>
      <Field label="Keep Ollama running" hint="Don't stop the app-managed server after runs (faster next run, uses memory)."><Switch checked={s.ollama_always_on} onChange={(ollama_always_on) => save({ ollama_always_on })} /></Field>
      <Field label="Log lines kept in memory per step" hint="Full logs always stream to disk."><NumberInput value={s.log_ring_lines} min={200} max={20000} step={500} onCommit={(v) => save({ log_ring_lines: v ?? 2000 })} /></Field>
    </Card>
  );
}

function WorkspaceSettings() {
  const workspace = useApp((s) => s.workspace);
  const setWorkspace = useApp((s) => s.setWorkspace);
  const settings = useApp((s) => s.settings)!;
  const [newCheck, setNewCheck] = useState("");
  if (!workspace) return <Card className="p-5 text-muted">Open a workspace to edit its settings.</Card>;
  const { row, info } = workspace;
  const save = async (patch: Partial<WorkspaceRow>) => setWorkspace({ row: await api.updateWorkspace({ ...row, ...patch }), info });
  const ws = row.settings;
  const checks = ws.check_commands ?? info.detected_checks;
  const setChecks = (c: string[] | null) => save({ settings: { ...ws, check_commands: c } });
  const tri = (v: boolean | null) => (v == null ? "inherit" : v ? "on" : "off");
  const fromTri = (v: string) => (v === "inherit" ? null : v === "on");
  return (
    <div className="space-y-4">
      <Card className="px-5">
        <Field label="Name"><InlineEdit value={row.display_name} className="text-[13px]" onSave={(display_name) => save({ display_name })} /></Field>
        <Field label="Folder"><span className="text-xs font-mono text-muted selectable">{shortPath(row.path)}</span></Field>
        <Field label="Default mode">
          <Select value={row.default_mode ?? ""} onChange={(e) => save({ default_mode: e.target.value || null })}>
            <option value="">Use global ({settings.modes.find((m) => m.id === settings.default_mode)?.display_name})</option>
            {settings.modes.map((m) => <option key={m.id} value={m.id}>{m.display_name}</option>)}
          </Select>
        </Field>
        <Field label="Local only" hint="Cloud models are removed from this workspace's executor pool; code never leaves your Mac (except to your paid agent)."><Switch checked={row.local_only} onChange={(local_only) => save({ local_only })} /></Field>
        <Field label="Uncommitted changes" hint="Default for runs started on a dirty working tree.">
          <Select value={ws.dirty_strategy ?? "stash"} onChange={(e) => save({ settings: { ...ws, dirty_strategy: e.target.value as DirtyStrategy } })}>
            <option value="stash">Stash</option><option value="commit">Commit as WIP</option><option value="run_on_top">Run on top</option>
          </Select>
        </Field>
        <Field label="Commit generated agent files" hint="By default files the Library generates (.claude/, .cursor/rules…) are git-ignored locally; only .orchestrator/library is suggested for commit."><Switch checked={ws.commit_generated} onChange={(commit_generated) => save({ settings: { ...ws, commit_generated } })} /></Field>
        <Field label="Approve before paid">
          <Select value={tri(ws.approve_before_paid)} onChange={(e) => save({ settings: { ...ws, approve_before_paid: fromTri(e.target.value) } })}><option value="inherit">Use global</option><option value="on">Always</option><option value="off">Never</option></Select>
        </Field>
        <Field label="Approve before cloud">
          <Select value={tri(ws.approve_before_cloud)} onChange={(e) => save({ settings: { ...ws, approve_before_cloud: fromTri(e.target.value) } })}><option value="inherit">Use global</option><option value="on">Always</option><option value="off">Never</option></Select>
        </Field>
      </Card>
      <div className="text-[11px] font-semibold uppercase tracking-wider text-faint">Check commands (verification gate)</div>
      <Card className="p-4 space-y-2">
        {checks.length === 0 && <div className="text-[12.5px] text-muted">No checks. Steps will be marked "unverified".</div>}
        {checks.map((c, i) => (
          <div key={i} className="flex items-center gap-2">
            <code className="flex-1 font-mono text-[12px] bg-panel-2 rounded px-2 py-1.5 selectable">{c}</code>
            <Button size="icon" variant="ghost" onClick={() => setChecks(checks.filter((_, j) => j !== i))}><X className="h-3.5 w-3.5" /></Button>
          </div>
        ))}
        <div className="flex gap-2">
          <Input placeholder="e.g. npm test" value={newCheck} onChange={(e) => setNewCheck(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && newCheck.trim()) { void setChecks([...checks, newCheck.trim()]); setNewCheck(""); } }} />
          <Button disabled={!newCheck.trim()} onClick={() => { void setChecks([...checks, newCheck.trim()]); setNewCheck(""); }}><Plus className="h-3.5 w-3.5" /> Add</Button>
        </div>
        {ws.check_commands && <Button size="sm" variant="ghost" onClick={() => setChecks(null)}><RotateCcw className="h-3.5 w-3.5" /> Use auto-detected ({info.detected_checks.join(", ") || "none"})</Button>}
      </Card>
      <div className="text-[11px] font-semibold uppercase tracking-wider text-faint">Budget cap for this workspace</div>
      <Card className="px-5">
        <BudgetFields b={ws.run_budget ?? settings.run_budget} onChange={(run_budget) => save({ settings: { ...ws, run_budget } })} />
      </Card>
    </div>
  );
}

function Prices() {
  const [s, save] = useSettings();
  const agents = useApp((st) => st.agents).filter((a) => a.family === "paid");
  return (
    <div className="space-y-3">
      <p className="text-[12.5px] text-muted">Subscription tools don't expose usage, so all costs are estimates. Set a price per million tokens, or an effective cost per request for subscription tools like Cursor. Cloud model prices come from the provider and are editable in Agents &amp; Models. Local models cost $0.</p>
      <Card className="px-5">
        {agents.map((a) => {
          const p = s.agent_prices[a.id] ?? {};
          const set = (patch: object) => save({ agent_prices: { ...s.agent_prices, [a.id]: { ...p, ...patch } } });
          return (
            <Field key={a.id} label={a.display_name} hint={p.per_request != null ? "Per request" : "Per million tokens (input / output)"}>
              <span className="text-xs text-muted">in $</span><NumberInput value={p.in_per_m} step={0.25} min={0} className="w-20" onCommit={(v) => set({ in_per_m: v })} />
              <span className="text-xs text-muted">out $</span><NumberInput value={p.out_per_m} step={0.25} min={0} className="w-20" onCommit={(v) => set({ out_per_m: v })} />
              <span className="text-xs text-muted">or per request $</span><NumberInput value={p.per_request} step={0.01} min={0} className="w-20" onCommit={(v) => set({ per_request: v })} />
            </Field>
          );
        })}
      </Card>
    </div>
  );
}

function Privacy() {
  const [s, save] = useSettings();
  return (
    <Card className="px-5">
      <Field label="Where data lives" hint="Runs, logs, settings and the Library stay on this Mac. The only data that leaves is what your chosen agents and cloud models send to their own providers."><Badge tone="ok">local</Badge></Field>
      <Field label="API keys" hint="Stored in the macOS Keychain, shown only as the last 4 characters, passed to agents through environment variables at run time."><Badge tone="ok">Keychain</Badge></Field>
      <Field label="Cloud notices acknowledged" hint="Before the first run on a provider, you confirm that workspace code is sent to it.">
        <span className="text-xs text-muted">{s.cloud_notice_ack.length ? s.cloud_notice_ack.join(", ") : "none"}</span>
        {s.cloud_notice_ack.length > 0 && <Button size="sm" variant="ghost" onClick={() => save({ cloud_notice_ack: [] })}>Reset</Button>}
      </Field>
      <Field label="Crash reporting"><Badge>off (not collected)</Badge></Field>
    </Card>
  );
}
