import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowRight,
  Check,
  ChevronDown,
  CornerDownLeft,
  FlaskConical,
  FolderOpen,
  GitBranch,
  Import,
  Lock,
  Pin,
  PinOff,
  Sparkles,
  Trash2,
} from "lucide-react";
import { useApp, modeName } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { DirtyStrategy, ExecutorEntry, ModeDef, PreflightView, RunRow, WorkspaceRow } from "../lib/types";
import { Badge, Button, Card, Kbd, Segmented, Switch, Textarea } from "../components/ui";
import { FitBadge, RunStatusBadge, TierBar } from "../components/domain";
import { ScreenHeader, pickFolder } from "../components/Shell";
import { ago, cn, shortPath, usd } from "../lib/format";

const EXAMPLES = ["Add unit tests for the untested functions", "Fix the failing tests", "Add input validation with clear error messages", "Write a README section on setup"];

export default function Home() {
  const workspace = useApp((s) => s.workspace);
  const settings = useApp((s) => s.settings)!;
  const agents = useApp((s) => s.agents);
  const runs = useApp((s) => s.runs);
  const setWorkspace = useApp((s) => s.setWorkspace);
  const viewRun = useApp((s) => s.viewRun);
  const toast = useApp((s) => s.toast);
  const go = useApp((s) => s.go);

  const [goal, setGoal] = useState("");
  const [mode, setMode] = useState<string>(settings.default_mode);
  const [pf, setPf] = useState<PreflightView | null>(null);
  const [override, setOverride] = useState<{ label: string; pool: ExecutorEntry[] } | null>(null);
  const [dirty, setDirty] = useState<DirtyStrategy>("stash");
  const [gitChoice, setGitChoice] = useState<"init" | "none">("init");
  const [unsafeOk, setUnsafeOk] = useState(false);
  const [starting, setStarting] = useState(false);
  const [recentRuns, setRecentRuns] = useState<RunRow[]>([]);
  const goalRef = useRef<HTMLTextAreaElement>(null);

  const ws = workspace?.row;
  const info = workspace?.info;

  useEffect(() => {
    setMode(ws?.default_mode ?? settings.default_mode);
    setDirty(ws?.settings.dirty_strategy ?? "stash");
    setOverride(null);
    setUnsafeOk(false);
  }, [ws?.id, ws?.default_mode, ws?.settings.dirty_strategy, settings.default_mode]);

  const refreshPf = useCallback(() => {
    api.preflight(ws?.id ?? null, override?.pool ?? null).then(setPf, () => setPf(null));
  }, [ws?.id, override]);
  useEffect(refreshPf, [refreshPf, settings.executor_pool, settings.memory_budget_pct]);

  useEffect(() => {
    if (!ws) return;
    api.listRuns(ws.id, null, 6).then(setRecentRuns, () => {});
  }, [ws?.id, runs, ws]);

  useEffect(() => {
    goalRef.current?.focus();
  }, [ws?.id]);

  const paidAgents = agents.filter((a) => a.family === "paid" && a.installed && a.enabled);
  const cheapAgents = agents.filter((a) => a.family === "open_source" && a.installed && a.enabled);
  const noPlanner = paidAgents.length === 0 && !settings.budget_planning;
  const poolEmpty = settings.executor_pool.filter((e) => e.enabled).length === 0;
  const activeHere = Object.values(runs).find((r) => r.workspace_id === ws?.id && ["pending", "planning", "running", "reviewing"].includes(r.status));
  const blocked = !ws || !goal.trim() || (info?.unsafe_root && !unsafeOk) || !!activeHere || !info?.writable || (noPlanner && poolEmpty);

  const start = async () => {
    if (blocked || !ws) return;
    setStarting(true);
    try {
      const id = await api.startRun({
        workspace_id: ws.id,
        goal: goal.trim(),
        mode,
        dirty_strategy: info?.dirty_count ? dirty : null,
        init_git: !info?.is_git && gitChoice === "init",
        pool_override: override?.pool ?? null,
      });
      setGoal("");
      viewRun(id);
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setStarting(false);
    }
  };

  if (!ws || !info) return <NoWorkspace />;

  return (
    <div className="flex flex-col h-full">
      <ScreenHeader title="Home" subtitle="Pick a folder, describe the work, choose how to spend.">
        <WorkspaceSwitcher />
      </ScreenHeader>
      <div className="flex-1 overflow-auto">
        <div className="max-w-[860px] mx-auto px-6 py-6 space-y-5">
          <WorkspaceNotices dirty={dirty} setDirty={setDirty} gitChoice={gitChoice} setGitChoice={setGitChoice} unsafeOk={unsafeOk} setUnsafeOk={setUnsafeOk} onChange={setWorkspace} />

          <Card className="p-4">
            <Textarea
              ref={goalRef}
              rows={4}
              value={goal}
              onChange={(e) => setGoal(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void start();
                }
              }}
              placeholder={`What should the agents do in ${ws.display_name}?`}
              className="text-[14px] border-0 focus:ring-0 px-1 bg-transparent"
            />
            <div className="flex items-center gap-2 mt-2 flex-wrap">
              {!goal &&
                EXAMPLES.map((ex) => (
                  <button key={ex} onClick={() => setGoal(ex)} className="h-6 px-2 rounded-md bg-panel-2 text-[11.5px] text-muted hover:text-fg border border-line">
                    {ex}
                  </button>
                ))}
            </div>
          </Card>

          <ModeCards value={mode} onChange={setMode} modes={settings.modes} />

          <Preflight pf={pf} override={override} setOverride={setOverride} />

          {(noPlanner || poolEmpty) && (
            <Card className="p-4 border-warn/30 bg-warn-soft/40">
              <div className="flex gap-3">
                <AlertTriangle className="h-4 w-4 text-warn mt-0.5 shrink-0" />
                <div className="flex-1 text-[12.5px] leading-relaxed">
                  {noPlanner && (
                    <p>
                      <b>No paid agent found.</b> Install Cursor Agent, Claude Code, Codex or Copilot, or turn on <b>budget planning</b> to let a strong cheap-cloud model plan.
                    </p>
                  )}
                  {poolEmpty && (
                    <p className={noPlanner ? "mt-1" : ""}>
                      <b>No cheap executor yet.</b> Connect a local model or a cheap cloud model to {cheapAgents[0]?.display_name ?? "OpenCode"} so routine steps cost less.
                    </p>
                  )}
                </div>
                <Button size="sm" onClick={() => go("agents")}>
                  Set up <ArrowRight className="h-3.5 w-3.5" />
                </Button>
              </div>
            </Card>
          )}

          <div className="flex items-center gap-3">
            <Button variant="primary" size="lg" onClick={start} disabled={!!blocked} loading={starting} className="px-5">
              <Sparkles className="h-4 w-4" />
              Run
            </Button>
            <span className="text-xs text-faint flex items-center gap-1">
              <Kbd>⌘</Kbd>
              <Kbd>
                <CornerDownLeft className="h-3 w-3" />
              </Kbd>
            </span>
            {activeHere && (
              <button className="text-xs text-info hover:underline" onClick={() => viewRun(activeHere.id)}>
                A run is active in this workspace. View it →
              </button>
            )}
          </div>

          {recentRuns.length > 0 && (
            <div className="pt-2">
              <div className="text-[11px] font-semibold uppercase tracking-wider text-faint mb-2">Recent runs here</div>
              <Card className="divide-y divide-line">
                {recentRuns.map((r) => (
                  <button key={r.id} onClick={() => viewRun(r.id)} className="w-full flex items-center gap-3 px-4 py-2.5 text-left hover:bg-panel-2 first:rounded-t-xl last:rounded-b-xl">
                    <RunStatusBadge status={r.status} />
                    <span className="flex-1 truncate">{r.goal}</span>
                    <span className="text-xs text-faint w-20 text-right">{modeName(settings, r.mode)}</span>
                    <span className="text-xs text-muted w-16 text-right">{usd(r.est_cost_usd, 2)}</span>
                    <span className="text-xs text-faint w-20 text-right">{ago(r.started_at)}</span>
                  </button>
                ))}
              </Card>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function NoWorkspace() {
  const openFolder = useApp((s) => s.openFolder);
  const setWorkspace = useApp((s) => s.setWorkspace);
  const recent = useApp((s) => s.recent);
  const toast = useApp((s) => s.toast);
  return (
    <div className="flex flex-col h-full">
      <ScreenHeader title="Home" />
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center max-w-md">
          <div className="mx-auto h-14 w-14 rounded-2xl bg-accent-soft flex items-center justify-center text-accent">
            <FolderOpen className="h-7 w-7" />
          </div>
          <h2 className="mt-4 text-lg font-semibold">Open a folder to start</h2>
          <p className="mt-1 text-muted">Agents work inside the folder you choose, like any agentic app. You can also drop a folder onto this window.</p>
          <div className="mt-5 flex justify-center gap-2">
            <Button variant="primary" size="lg" onClick={async () => {
              const p = await pickFolder();
              if (p) await openFolder(p);
            }}>
              <FolderOpen className="h-4 w-4" /> Open folder…
            </Button>
            <Button size="lg" onClick={async () => {
              try {
                setWorkspace(await api.sampleProject());
              } catch (e) {
                toast("error", errorText(e));
              }
            }}>
              <FlaskConical className="h-4 w-4" /> Try the sample project
            </Button>
          </div>
          {recent.length > 0 && (
            <div className="mt-6 text-left">
              <div className="text-[11px] font-semibold uppercase tracking-wider text-faint mb-2">Recent</div>
              {recent.slice(0, 5).map((r) => (
                <button key={r.id} onClick={() => openFolder(r.path)} className="w-full flex items-center gap-2 px-3 h-9 rounded-lg hover:bg-panel-2 text-left">
                  <FolderOpen className="h-4 w-4 text-faint" />
                  <span className="font-medium">{r.display_name}</span>
                  <span className="text-xs text-faint truncate">{shortPath(r.path)}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export function WorkspaceSwitcher() {
  const workspace = useApp((s) => s.workspace);
  const recent = useApp((s) => s.recent);
  const openFolder = useApp((s) => s.openFolder);
  const refreshRecent = useApp((s) => s.refreshRecent);
  const setWorkspace = useApp((s) => s.setWorkspace);
  const toast = useApp((s) => s.toast);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [open]);
  const pin = async (r: WorkspaceRow) => {
    await api.updateWorkspace({ ...r, pinned: !r.pinned });
    void refreshRecent();
  };
  return (
    <div ref={ref} className="relative no-drag">
      <Button onClick={() => setOpen(!open)} className="max-w-[280px]">
        <FolderOpen className="h-4 w-4 text-faint" />
        <span className="truncate">{workspace?.row.display_name ?? "Open folder"}</span>
        <ChevronDown className="h-3.5 w-3.5 text-faint" />
      </Button>
      {open && (
        <div className="absolute right-0 top-9 z-30 w-[360px] rounded-xl border border-line bg-elev shadow-xl p-1.5 animate-fade-in">
          <button
            className="w-full flex items-center gap-2 h-8 px-2.5 rounded-lg hover:bg-panel-2 text-left font-medium"
            onClick={async () => {
              setOpen(false);
              const p = await pickFolder();
              if (p) await openFolder(p);
            }}
          >
            <FolderOpen className="h-4 w-4" /> Open folder… <span className="ml-auto"><Kbd>⌘O</Kbd></span>
          </button>
          <button
            className="w-full flex items-center gap-2 h-8 px-2.5 rounded-lg hover:bg-panel-2 text-left"
            onClick={async () => {
              setOpen(false);
              try {
                setWorkspace(await api.sampleProject());
              } catch (e) {
                toast("error", errorText(e));
              }
            }}
          >
            <FlaskConical className="h-4 w-4" /> Sample project
          </button>
          {recent.length > 0 && <div className="px-2.5 pt-2 pb-1 text-[11px] font-semibold uppercase tracking-wider text-faint">Recent</div>}
          {recent.map((r) => (
            <div key={r.id} className={cn("group flex items-center gap-2 h-9 px-2.5 rounded-lg hover:bg-panel-2", workspace?.row.id === r.id && "bg-panel-2")}>
              <button className="flex-1 min-w-0 text-left" onClick={() => { setOpen(false); void openFolder(r.path); }}>
                <div className="flex items-center gap-1.5">
                  {r.is_git && <GitBranch className="h-3 w-3 text-faint" />}
                  <span className="truncate font-medium">{r.display_name}</span>
                  {r.local_only && <Lock className="h-3 w-3 text-faint" />}
                </div>
                <div className="text-[11px] text-faint truncate">{shortPath(r.path)}</div>
              </button>
              <button className={cn("p-1 rounded text-faint hover:text-fg", r.pinned ? "opacity-100" : "opacity-0 group-hover:opacity-100")} onClick={() => pin(r)} title={r.pinned ? "Unpin" : "Pin"}>
                {r.pinned ? <PinOff className="h-3.5 w-3.5" /> : <Pin className="h-3.5 w-3.5" />}
              </button>
              <button className="p-1 rounded text-faint hover:text-bad opacity-0 group-hover:opacity-100" title="Remove from recent" onClick={async () => { await api.removeWorkspace(r.id); void refreshRecent(); }}>
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function WorkspaceNotices({
  dirty,
  setDirty,
  gitChoice,
  setGitChoice,
  unsafeOk,
  setUnsafeOk,
  onChange,
}: {
  dirty: DirtyStrategy;
  setDirty: (d: DirtyStrategy) => void;
  gitChoice: "init" | "none";
  setGitChoice: (g: "init" | "none") => void;
  unsafeOk: boolean;
  setUnsafeOk: (b: boolean) => void;
  onChange: (w: ReturnType<typeof useApp.getState>["workspace"]) => void;
}) {
  const w = useApp((s) => s.workspace)!;
  const brand = useApp((s) => s.brand);
  const toast = useApp((s) => s.toast);
  const { row, info } = w;
  const [importing, setImporting] = useState(false);
  const save = async (patch: Partial<WorkspaceRow>) => {
    const r = await api.updateWorkspace({ ...row, ...patch });
    onChange({ row: r, info });
  };
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3 text-[12.5px] text-muted flex-wrap">
        <span className="selectable font-mono text-[11.5px] truncate max-w-[420px]" title={row.path}>{shortPath(row.path)}</span>
        {info.is_git ? (
          <Badge tone="neutral"><GitBranch className="h-3 w-3" /> {info.branch ?? "git"}</Badge>
        ) : (
          <Badge tone="warn">not a git repo</Badge>
        )}
        {info.detected_checks.length > 0 ? (
          <Badge tone="ok" title={info.detected_checks.join("\n")}><Check className="h-3 w-3" /> checks: {(row.settings.check_commands ?? info.detected_checks).join(", ")}</Badge>
        ) : (
          !row.settings.check_commands?.length && <Badge tone="neutral" title="Add check commands in Settings → Workspace">no checks detected</Badge>
        )}
        <span className="ml-auto flex items-center gap-2" title="Keep this workspace on your Mac: cloud models are removed from its executor pool">
          <Lock className="h-3.5 w-3.5" /> Local only
          <Switch checked={row.local_only} onChange={(v) => save({ local_only: v })} />
        </span>
      </div>

      {!info.writable && (
        <Card className="p-3 border-bad/30 bg-bad-soft/50 text-[12.5px] text-bad">This folder isn't writable. Choose another folder.</Card>
      )}

      {info.unsafe_root && (
        <Card className="p-3 border-warn/30 bg-warn-soft/50 flex items-center gap-3 text-[12.5px]">
          <AlertTriangle className="h-4 w-4 text-warn shrink-0" />
          <span className="flex-1">This is your home folder or a system folder. Agents could change files far outside a project.</span>
          <label className="flex items-center gap-2 font-medium">
            <Switch checked={unsafeOk} onChange={setUnsafeOk} /> I understand, allow it
          </label>
        </Card>
      )}

      {!info.is_git && (
        <Card className="p-3 flex items-center gap-3 text-[12.5px]">
          <GitBranch className="h-4 w-4 text-faint shrink-0" />
          <span className="flex-1">Not a git repo. Git gives every step its own commit so you can roll back cleanly.</span>
          <Segmented
            size="sm"
            value={gitChoice}
            onChange={setGitChoice}
            options={[
              { value: "init", label: "Initialise git" },
              { value: "none", label: "Use snapshots", title: "Run without git; each step's files are snapshotted for rollback" },
            ]}
          />
        </Card>
      )}

      {info.is_git && info.dirty_count > 0 && (
        <Card className="p-3 flex items-center gap-3 text-[12.5px]">
          <AlertTriangle className="h-4 w-4 text-warn shrink-0" />
          <span className="flex-1" title={info.dirty_files.join("\n")}>
            {info.dirty_count} uncommitted change{info.dirty_count === 1 ? "" : "s"}. The run works on its own branch.
          </span>
          <Segmented
            size="sm"
            value={dirty}
            onChange={setDirty}
            options={[
              { value: "stash", label: "Stash", title: "Set them aside; restore after the run" },
              { value: "commit", label: "Commit", title: "Commit them as WIP first" },
              { value: "run_on_top", label: "Run on top", title: "Keep them in the working tree" },
            ]}
          />
        </Card>
      )}

      {info.importable.length > 0 && !row.settings.import_offered && (
        <Card className="p-3 flex items-center gap-3 text-[12.5px]">
          <Import className="h-4 w-4 text-accent shrink-0" />
          <span className="flex-1">
            Found existing agent config: <b>{info.importable.join(", ")}</b>. Import it into the {brand.shortName} Library so every agent gets it?
          </span>
          <Button size="sm" variant="ghost" onClick={() => save({ settings: { ...row.settings, import_offered: true } })}>Not now</Button>
          <Button
            size="sm"
            variant="primary"
            loading={importing}
            onClick={async () => {
              setImporting(true);
              try {
                const n = await api.libraryImport(row.id, "workspace");
                toast("info", `Imported ${n} item${n === 1 ? "" : "s"} into this workspace's Library.`);
                onChange({ row: { ...row, settings: { ...row.settings, import_offered: true } }, info });
              } catch (e) {
                toast("error", errorText(e));
              } finally {
                setImporting(false);
              }
            }}
          >
            Import
          </Button>
        </Card>
      )}
    </div>
  );
}

const EXAMPLE_SHARE: Record<string, [number, number, number, string]> = {
  cost: [70, 25, 5, "Cheapest. Paid agent only plans."],
  balanced: [45, 20, 35, "Paid for hard steps, cheap for routine."],
  intelligent: [10, 10, 80, "Best quality; cheap only for chores."],
};

export function ModeCards({ value, onChange, modes }: { value: string; onChange: (m: string) => void; modes: ModeDef[] }) {
  return (
    <div className="grid gap-3" style={{ gridTemplateColumns: `repeat(${Math.min(modes.length, 3)}, minmax(0, 1fr))` }}>
      {modes.map((m) => {
        const ex = EXAMPLE_SHARE[m.id] ?? derivedShare(m);
        const sel = value === m.id;
        return (
          <button
            key={m.id}
            onClick={() => onChange(m.id)}
            className={cn(
              "text-left rounded-xl border p-3.5 transition-all",
              sel ? "border-accent bg-accent-soft/40 ring-2 ring-accent/20" : "border-line bg-panel hover:border-line-strong",
            )}
          >
            <div className="flex items-center justify-between">
              <span className="font-semibold text-[13.5px]">{m.display_name}</span>
              {sel && <Check className="h-4 w-4 text-accent" />}
            </div>
            <div className="text-[12px] text-muted mt-1 leading-snug min-h-[32px]">{ex[3]}</div>
            <TierBar local={ex[0]} cloud={ex[1]} premium={ex[2]} className="mt-3" />
            <div className="flex justify-between text-[10.5px] text-faint mt-1">
              <span>cheap</span>
              <span>paid</span>
            </div>
          </button>
        );
      })}
    </div>
  );
}

function derivedShare(m: ModeDef): [number, number, number, string] {
  const paid = [m.high, m.low, m.trivial].filter((x) => x === "paid").length;
  return [60 - paid * 18, 20, 20 + paid * 18, m.description];
}

function Preflight({ pf, override, setOverride }: { pf: PreflightView | null; override: { label: string; pool: ExecutorEntry[] } | null; setOverride: (o: { label: string; pool: ExecutorEntry[] } | null) => void }) {
  const settings = useApp((s) => s.settings)!;
  const label = (e: ExecutorEntry) => e.model.display_name ?? e.model.name;
  const options = useMemo(() => {
    if (!pf || pf.fit === "fits" || pf.fit === null) return [];
    const o: { label: string; pool: ExecutorEntry[] }[] = [];
    if (pf.smaller[0]) o.push({ label: `Smaller model: ${label(pf.smaller[0])}`, pool: [pf.smaller[0], ...pf.cloud] });
    if (pf.cloud[0]) o.push({ label: `Cloud for this run: ${label(pf.cloud[0])}`, pool: pf.cloud });
    return o;
  }, [pf]);
  if (!pf) return null;
  return (
    <Card className="p-3.5">
      <div className="flex items-start gap-3">
        <div className="pt-0.5">{pf.fit ? <FitBadge fit={pf.fit} /> : <Badge tone="cloud">Cloud</Badge>}</div>
        <div className="flex-1 min-w-0">
          <div className="text-[12.5px] leading-relaxed">{override ? `This run uses ${override.label.toLowerCase()}.` : pf.line}</div>
          {pf.notes.map((n) => (
            <div key={n} className="text-[12px] text-muted mt-0.5">{n}</div>
          ))}
          {!override && options.length > 0 && (
            <div className="flex gap-2 mt-2 flex-wrap">
              {options.map((o) => (
                <Button key={o.label} size="sm" onClick={() => setOverride(o)}>
                  {o.label}
                </Button>
              ))}
              <Button size="sm" variant="ghost">Proceed anyway</Button>
            </div>
          )}
          {override && (
            <Button size="sm" variant="ghost" className="mt-1 -ml-2" onClick={() => setOverride(null)}>
              Use the normal executor pool
            </Button>
          )}
        </div>
        <div className="text-[11px] text-faint text-right shrink-0">
          budget {pf.budget_gb.toFixed(1)} GB
          <br />
          {settings.executor_pool.filter((e) => e.enabled).length} executor{settings.executor_pool.length === 1 ? "" : "s"}
        </div>
      </div>
    </Card>
  );
}
