import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  ArrowDownToLine,
  Ban,
  Check,
  ChevronLeft,
  Download,
  FileText,
  GitBranch,
  GitMerge,
  Pause,
  Play,
  RotateCcw,
  Shuffle,
  Sparkles,
  UserRound,
} from "lucide-react";
import { useApp, agentName, modeName } from "../lib/store";
import { api, errorText } from "../lib/api";
import { logStore, useStepLogs } from "../lib/logs";
import type { ExecutorEntry, LibraryItem, LogLine, ModelRef, RunRow, StepRow } from "../lib/types";
import { Badge, Button, Card, Dialog, Empty, Select, Segmented, Switch } from "../components/ui";
import { ClassBadge, RunStatusBadge, StepIcon, TierBadge, TierBar, stepStatusLabel } from "../components/domain";
import { ScreenHeader } from "../components/Shell";
import { cn, duration, mb, tokens, usd } from "../lib/format";

const LIVE = ["pending", "planning", "running", "reviewing"];

export default function RunView() {
  const runId = useApp((s) => s.viewRunId);
  const run = useApp((s) => (runId ? s.runs[runId] : undefined));
  const stepsMap = useApp((s) => (runId ? s.steps[runId] : undefined));
  const loadRun = useApp((s) => s.loadRun);
  const go = useApp((s) => s.go);
  const [selected, setSelected] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);

  useEffect(() => {
    if (runId) void loadRun(runId);
    if (runId) api.getRun(runId).then((d) => setPaused(d.paused), () => {});
  }, [runId, loadRun]);

  const steps = useMemo(() => Object.values(stepsMap ?? {}).sort((a, b) => kindOrder(a) - kindOrder(b) || a.idx - b.idx), [stepsMap]);

  // Follow the active step unless the user picked one.
  const [pinned, setPinned] = useState(false);
  useEffect(() => {
    if (pinned) return;
    const active = steps.find((s) => ["running", "verifying", "awaiting_approval"].includes(s.status)) ?? steps.filter((s) => s.attempts > 0).pop() ?? steps[0];
    if (active && active.id !== selected) setSelected(active.id);
  }, [steps, pinned, selected]);

  if (!runId || !run) {
    return (
      <div className="flex flex-col h-full">
        <ScreenHeader title="Run" />
        <Empty title="No run selected" action={<Button onClick={() => go("home")}>Back to Home</Button>} />
      </div>
    );
  }

  const step = steps.find((s) => s.id === selected) ?? null;
  return (
    <div className="flex flex-col h-full">
      <RunHeader run={run} paused={paused} setPaused={setPaused} />
      {run.summary.error && run.status === "failed" && (
        <div className="px-6 py-2.5 bg-bad-soft text-bad border-b border-bad/20 text-[12.5px] selectable">
          <b>Run stopped:</b> {run.summary.error}
        </div>
      )}
      {run.summary.restored && (
        <div className="px-6 py-2 bg-info-soft text-info border-b border-info/20 text-[12.5px]">
          Nothing was changed, so the workspace was put back on {run.base_ref ?? "its branch"}{run.summary.restored_stash ? " with your uncommitted changes restored" : ""}.
        </div>
      )}
      <Totals run={run} steps={steps} />
      <div className="flex-1 flex min-h-0">
        <div className="w-[360px] shrink-0 border-r border-line overflow-auto p-3 space-y-1">
          {steps.length === 0 && <div className="text-muted text-[12.5px] p-3">Preparing the workspace…</div>}
          {steps.map((s) => (
            <StepItem key={s.id} s={s} selected={s.id === selected} onClick={() => { setSelected(s.id); setPinned(true); }} />
          ))}
          {pinned && LIVE.includes(run.status) && (
            <button className="w-full text-xs text-info py-2 hover:underline" onClick={() => setPinned(false)}>Follow the active step</button>
          )}
        </div>
        <div className="flex-1 min-w-0 flex flex-col">{step ? <StepDetail run={run} step={step} paused={paused} /> : <Empty title="Select a step" />}</div>
      </div>
    </div>
  );
}

function kindOrder(s: StepRow) {
  return s.kind === "plan" ? 0 : s.kind === "execute" ? 1 : 2;
}

function RunHeader({ run, paused, setPaused }: { run: RunRow; paused: boolean; setPaused: (b: boolean) => void }) {
  const settings = useApp((s) => s.settings)!;
  const go = useApp((s) => s.go);
  const toast = useApp((s) => s.toast);
  const live = LIVE.includes(run.status);
  const [accept, setAccept] = useState(false);
  return (
    <ScreenHeader
      title={
        <span className="flex items-center gap-2">
          <button onClick={() => go("home")} className="text-faint hover:text-fg no-drag" aria-label="Back"><ChevronLeft className="h-4 w-4" /></button>
          <span className="truncate">{run.goal}</span>
        </span>
      }
      subtitle={
        <span className="flex items-center gap-2 pl-6">
          {run.branch && <span className="flex items-center gap-1"><GitBranch className="h-3 w-3" />{run.branch}</span>}
          {run.budget_planning && <Badge tone="cloud">Budget planning</Badge>}
          {run.summary.review_verdict && <span>· {run.summary.review_verdict}</span>}
        </span>
      }
    >
      <RunStatusBadge status={run.status} paused={paused} />
      {live ? (
        <>
          <Select
            className="no-drag"
            value={run.mode}
            title="Mode for the remaining steps"
            onChange={async (e) => {
              try {
                await api.setRunMode(run.id, e.target.value);
              } catch (err) {
                toast("error", errorText(err));
              }
            }}
          >
            {settings.modes.map((m) => <option key={m.id} value={m.id}>{m.display_name}</option>)}
          </Select>
          <Button className="no-drag" onClick={async () => { await api.pauseRun(run.id, !paused); setPaused(!paused); }}>
            {paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
            {paused ? "Resume" : "Pause"}
          </Button>
          <Button className="no-drag text-bad" variant="outline" onClick={() => api.cancelRun(run.id)}>
            <Ban className="h-3.5 w-3.5" /> Cancel
          </Button>
        </>
      ) : (
        <>
          <span className="text-xs text-muted">{modeName(settings, run.mode)}</span>
          <Button
            className="no-drag"
            variant="ghost"
            onClick={async () => {
              const path = await save({ defaultPath: `run-${run.id.slice(0, 8)}.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
              if (path) {
                await api.exportRun(run.id, path);
                toast("info", "Run exported.");
              }
            }}
          >
            <Download className="h-3.5 w-3.5" /> Export
          </Button>
          {!run.summary.accepted && run.status !== "cancelled" && (
            <Button className="no-drag" variant="primary" onClick={() => setAccept(true)}>
              <Check className="h-3.5 w-3.5" /> Accept run
            </Button>
          )}
        </>
      )}
      <AcceptDialog run={run} open={accept} onClose={() => setAccept(false)} />
    </ScreenHeader>
  );
}

function AcceptDialog({ run, open, onClose }: { run: RunRow; open: boolean; onClose: () => void }) {
  const toast = useApp((s) => s.toast);
  const [merge, setMerge] = useState(true);
  const [stash, setStash] = useState(true);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Accept this run"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={busy}
            onClick={async () => {
              setBusy(true);
              try {
                toast("info", await api.acceptRun(run.id, !!run.branch && merge, stash));
                onClose();
              } catch (e) {
                toast("error", errorText(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            Accept
          </Button>
        </>
      }
    >
      <div className="space-y-3 text-[12.5px] text-muted">
        <p>Accepting clears this run's rollback snapshots.</p>
        {run.branch && run.base_ref && (
          <label className="flex items-center justify-between gap-3 text-fg">
            <span className="flex items-center gap-2"><GitMerge className="h-4 w-4" /> Merge <code>{run.branch}</code> into <code>{run.base_ref}</code></span>
            <Switch checked={merge} onChange={setMerge} />
          </label>
        )}
        {run.summary.stashed && (
          <label className="flex items-center justify-between gap-3 text-fg">
            <span>Restore the changes stashed before the run</span>
            <Switch checked={stash} onChange={setStash} />
          </label>
        )}
      </div>
    </Dialog>
  );
}

function useTicker(active: boolean) {
  const [, setT] = useState(0);
  useEffect(() => {
    if (!active) return;
    const id = setInterval(() => setT((x) => x + 1), 1000);
    return () => clearInterval(id);
  }, [active]);
}

function Totals({ run, steps }: { run: RunRow; steps: StepRow[] }) {
  const live = LIVE.includes(run.status);
  useTicker(live);
  const exec = steps.filter((s) => s.kind === "execute");
  const done = exec.filter((s) => s.status === "passed" || s.status === "accepted").length;
  const used = steps.filter((s) => s.attempts > 0);
  const n = (t: string) => used.filter((s) => s.tier === t).length;
  const cost = (t: string) => steps.filter((s) => s.tier === t).reduce((a, s) => a + s.cost_usd, 0);
  const total = steps.reduce((a, s) => a + s.cost_usd, 0);
  const saved = steps.filter((s) => s.tier !== "premium").reduce((a, s) => a + Math.max(0, s.paid_equiv_usd - s.cost_usd), 0);
  const tin = steps.reduce((a, s) => a + s.tokens_in, 0);
  const tout = steps.reduce((a, s) => a + s.tokens_out, 0);
  const est = steps.some((s) => s.tokens_estimated);
  const elapsed = (run.ended_at ?? Date.now()) - run.started_at;
  return (
    <div className="grid grid-cols-5 gap-px bg-line border-b border-line shrink-0">
      <Stat label="Steps" value={`${done}/${exec.length || "–"}`} sub={exec.some((s) => s.escalated) ? `${exec.filter((s) => s.escalated).length} escalated` : undefined} />
      <Stat
        label="Tiers"
        value={
          <span className="flex items-center gap-2 text-[12.5px]">
            <span className="text-local">{n("local")} local</span>
            <span className="text-cloud">{n("cheap_cloud")} cloud</span>
            <span className="text-premium">{n("premium")} paid</span>
          </span>
        }
        sub={<TierBar local={n("local")} cloud={n("cheap_cloud")} premium={n("premium")} className="mt-1" />}
      />
      <Stat label={`Tokens${est ? " (est.)" : ""}`} value={`${tokens(tin)} in · ${tokens(tout)} out`} />
      <Stat
        label="Est. cost"
        value={usd(total)}
        sub={<span title={`Premium ${usd(cost("premium"))} · Cheap cloud ${usd(cost("cheap_cloud"))} · Local $0`}>saved ~{usd(saved, 2)} vs all-paid</span>}
      />
      <Stat label="Time" value={duration(elapsed)} sub={run.peak_mem_mb > 0 ? `peak ${mb(run.peak_mem_mb)}` : undefined} />
    </div>
  );
}

function Stat({ label, value, sub }: { label: string; value: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <div className="bg-panel px-4 py-2.5 min-w-0">
      <div className="text-[10.5px] uppercase tracking-wider font-semibold text-faint">{label}</div>
      <div className="text-[14px] font-semibold mt-0.5 truncate">{value}</div>
      {sub && <div className="text-[11px] text-muted truncate">{sub}</div>}
    </div>
  );
}

function StepItem({ s, selected, onClick }: { s: StepRow; selected: boolean; onClick: () => void }) {
  const agents = useApp((st) => st.agents);
  const exec = s.detail.executor ?? agentName(agents, s.agent_id);
  return (
    <button
      onClick={onClick}
      className={cn("w-full text-left rounded-lg px-3 py-2.5 border transition-colors", selected ? "bg-panel border-line-strong shadow-sm" : "border-transparent hover:bg-panel-2")}
    >
      <div className="flex items-start gap-2">
        <StepIcon status={s.status} className="mt-0.5" />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="text-faint text-xs tabular-nums">{s.kind === "plan" ? "P" : s.kind === "review" ? "R" : s.idx}</span>
            <span className="font-medium truncate">{s.title}</span>
          </div>
          <div className="flex items-center gap-1.5 mt-1 flex-wrap">
            <TierBadge tier={s.tier} />
            {s.kind === "execute" && <ClassBadge cls={s.class} />}
            {s.library_agent_id && <Badge tone="accent"><UserRound className="h-3 w-3" />{s.library_agent_id}</Badge>}
            {s.escalated && <Badge tone="warn">escalated</Badge>}
            {s.attempts > 1 && <Badge>×{s.attempts}</Badge>}
          </div>
          <div className="text-[11.5px] text-muted mt-1 truncate" title={s.route_reason ?? undefined}>{exec}</div>
        </div>
        <span className={cn("text-[11px] shrink-0", s.status === "failed" ? "text-bad" : s.status === "awaiting_approval" ? "text-warn" : "text-faint")}>
          {stepStatusLabel[s.status]}
        </span>
      </div>
    </button>
  );
}

function StepDetail({ run, step, paused }: { run: RunRow; step: StepRow; paused: boolean }) {
  const agents = useApp((s) => s.agents);
  const toast = useApp((s) => s.toast);
  const dataDir = useApp((s) => s.dataDir);
  const [reassign, setReassign] = useState(false);
  const live = LIVE.includes(run.status);
  const canRollback = (!live || paused) && (step.status === "passed" || step.status === "accepted") && step.kind !== "plan";

  // Backfill this step's log from the core (memory or disk).
  useEffect(() => {
    api.stepLogs(run.id, step.id, step.idx, 0).then((l) => logStore.backfill(step.id, l), () => {});
  }, [run.id, step.id, step.idx]);

  return (
    <>
      <div className="px-5 py-3.5 border-b border-line space-y-2 shrink-0">
        <div className="flex items-center gap-2">
          <StepIcon status={step.status} />
          <span className="font-semibold text-[14px] truncate">{step.title}</span>
          <span className="ml-auto flex gap-2">
            {step.status === "pending" && live && (
              <Button size="sm" onClick={() => setReassign(true)}><Shuffle className="h-3.5 w-3.5" /> Reassign</Button>
            )}
            {canRollback && (
              <Button
                size="sm"
                variant="outline"
                onClick={async () => {
                  try {
                    const n = await api.rollbackStep(run.id, step.idx);
                    toast("info", `Rolled back ${n} step${n === 1 ? "" : "s"}.`);
                  } catch (e) {
                    toast("error", errorText(e));
                  }
                }}
              >
                <RotateCcw className="h-3.5 w-3.5" /> Roll back to before this step
              </Button>
            )}
            <Button size="sm" variant="ghost" title="Show the full log file" onClick={() => revealItemInDir(`${dataDir}/logs/${run.id}/${step.idx}.log`).catch(() => toast("info", "No log file yet."))}>
              <FileText className="h-3.5 w-3.5" />
            </Button>
          </span>
        </div>
        <div className="flex items-center gap-2 flex-wrap text-[12px]">
          <TierBadge tier={step.tier} />
          <span className="text-muted">{step.detail.executor ?? agentName(agents, step.agent_id)}</span>
          {step.library_agent_id && <Badge tone="accent"><UserRound className="h-3 w-3" /> as {step.library_agent_id}</Badge>}
          {step.attempts > 0 && <span className="text-faint">· {step.attempts} attempt{step.attempts === 1 ? "" : "s"}</span>}
          {step.cost_usd > 0 && <span className="text-faint">· {usd(step.cost_usd)}</span>}
          {(step.tokens_in > 0 || step.tokens_out > 0) && <span className="text-faint">· {tokens(step.tokens_in)}/{tokens(step.tokens_out)} tok{step.tokens_estimated ? " (est.)" : ""}</span>}
          {step.detail.unverified && <Badge tone="warn" title="No check commands ran for this step">unverified</Badge>}
        </div>
        {step.route_reason && (
          <div className="text-[12px] text-muted flex gap-1.5"><Sparkles className="h-3.5 w-3.5 mt-0.5 text-accent shrink-0" /><span className="selectable">{step.route_reason}{step.detail.class_reason ? ` · ${step.detail.class_reason}` : ""}</span></div>
        )}
        {step.detail.agent_error && ["failed", "cancelled", "awaiting_approval"].includes(step.status) && (
          <div className="text-[12.5px] selectable bg-bad-soft text-bad rounded-lg px-3 py-2 whitespace-pre-wrap">{step.detail.agent_error}</div>
        )}
        {step.detail.summary && <div className="text-[12.5px] selectable bg-panel-2 rounded-lg px-3 py-2">{step.detail.summary}</div>}
        {step.detail.changed && step.detail.changed.length > 0 && (
          <div className="text-[11.5px] text-muted font-mono truncate" title={step.detail.changed.join("\n")}>changed: {step.detail.changed.join(", ")}</div>
        )}
        {step.kind === "review" && step.detail.review && (
          <pre className="text-[12px] whitespace-pre-wrap bg-panel-2 rounded-lg px-3 py-2 max-h-40 overflow-auto">{step.detail.review}</pre>
        )}
      </div>
      <LogView stepId={step.id} live={["running", "verifying"].includes(step.status)} />
      {reassign && <ReassignDialog run={run} step={step} onClose={() => setReassign(false)} />}
    </>
  );
}

type LogFilter = "all" | "out" | "tool" | "check" | "error";

function LogView({ stepId, live }: { stepId: string; live: boolean }) {
  const [lines, version] = useStepLogs(stepId);
  const [filter, setFilter] = useState<LogFilter>("all");
  const [follow, setFollow] = useState(true);
  const parent = useRef<HTMLDivElement>(null);
  const shown = useMemo(() => {
    if (filter === "all") return lines;
    if (filter === "tool") return lines.filter((l) => l.kind === "tool" || l.kind === "edit");
    if (filter === "check") return lines.filter((l) => l.kind === "check");
    if (filter === "error") return lines.filter((l) => l.kind === "error" || l.kind === "governor");
    return lines.filter((l) => l.kind === "out");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lines, version, filter]);
  const v = useVirtualizer({ count: shown.length, getScrollElement: () => parent.current, estimateSize: () => 19, overscan: 30 });
  useEffect(() => {
    if (follow && shown.length) v.scrollToIndex(shown.length - 1, { align: "end" });
  }, [shown.length, follow, v, version]);
  return (
    <div className="flex-1 min-h-0 flex flex-col">
      <div className="flex items-center gap-2 px-4 py-2 border-b border-line shrink-0">
        <Segmented
          size="sm"
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: "All" },
            { value: "out", label: "Output" },
            { value: "tool", label: "Tools & edits" },
            { value: "check", label: "Checks" },
            { value: "error", label: "Errors & governor" },
          ]}
        />
        <span className="ml-auto text-[11px] text-faint">{lines.length.toLocaleString()} lines{lines.length >= 2000 ? " (latest 2,000)" : ""}</span>
        <Button size="sm" variant={follow ? "secondary" : "ghost"} onClick={() => setFollow(!follow)} title="Follow the newest lines">
          <ArrowDownToLine className="h-3.5 w-3.5" />
        </Button>
      </div>
      <div
        ref={parent}
        className="flex-1 overflow-auto font-mono text-[11.5px] leading-[19px] selectable bg-panel"
        onWheel={(e) => e.deltaY < 0 && follow && setFollow(false)}
      >
        {shown.length === 0 ? (
          <div className="p-4 text-faint font-sans text-[12.5px]">{live ? "Waiting for output…" : "No output."}</div>
        ) : (
          <div style={{ height: v.getTotalSize(), position: "relative" }}>
            {v.getVirtualItems().map((it) => (
              <LogRow key={it.key} line={shown[it.index]} top={it.start} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

const kindStyle: Record<LogLine["kind"], string> = {
  out: "text-fg",
  tool: "text-info",
  edit: "text-local",
  error: "text-bad",
  governor: "text-warn",
  check: "text-muted",
  route: "text-premium",
  info: "text-muted",
};
const kindTag: Record<LogLine["kind"], string> = { out: "", tool: "tool", edit: "edit", error: "err", governor: "gov", check: "chk", route: "route", info: "info" };

function LogRow({ line, top }: { line: LogLine; top: number }) {
  return (
    <div className={cn("absolute left-0 right-0 px-4 whitespace-pre overflow-hidden text-ellipsis", kindStyle[line.kind])} style={{ top, height: 19 }} title={line.text.length > 120 ? line.text : undefined}>
      {kindTag[line.kind] && <span className="inline-block w-11 text-faint">{kindTag[line.kind]}</span>}
      {line.text}
    </div>
  );
}

function ReassignDialog({ run, step, onClose }: { run: RunRow; step: StepRow; onClose: () => void }) {
  const agents = useApp((s) => s.agents);
  const settings = useApp((s) => s.settings)!;
  const toast = useApp((s) => s.toast);
  const workspace = useApp((s) => s.workspace);
  const [library, setLibrary] = useState<LibraryItem[]>([]);
  const usable = agents.filter((a) => a.installed && a.enabled);
  const options = useMemo(() => {
    const o: { key: string; label: string; agent: string; model: ModelRef | null }[] = [];
    for (const a of usable.filter((x) => x.family === "paid")) o.push({ key: `paid:${a.id}`, label: `${a.display_name} (premium)`, agent: a.id, model: null });
    const pool: ExecutorEntry[] = [...settings.executor_pool, ...(settings.budget_planner ? [settings.budget_planner] : [])];
    for (const e of pool) {
      const a = agents.find((x) => x.id === e.agent_id);
      o.push({ key: `pool:${e.agent_id}:${e.model.provider_id}:${e.model.name}`, label: `${a?.display_name ?? e.agent_id} · ${e.model.display_name ?? e.model.name} (${e.model.tier === "local" ? "local" : "cheap cloud"})`, agent: e.agent_id, model: e.model });
    }
    return o;
  }, [usable, settings, agents]);
  const [choice, setChoice] = useState(options[0]?.key ?? "");
  const [libAgent, setLibAgent] = useState<string>(step.library_agent_id ?? "");
  useEffect(() => {
    api.libraryList(workspace?.row.id ?? null).then((v) => setLibrary(v.items.filter((i) => i.kind === "agent" && i.enabled && !i.overridden)), () => {});
  }, [workspace?.row.id]);
  const submit = async () => {
    const o = options.find((x) => x.key === choice);
    if (!o) return;
    try {
      await api.reassignStep(run.id, step.id, { agent_id: o.agent, model: o.model, library_agent: libAgent || null });
      toast("info", `Step ${step.idx} reassigned.`);
      onClose();
    } catch (e) {
      toast("error", errorText(e));
    }
  };
  return (
    <Dialog open onClose={onClose} title={`Reassign step ${step.idx}`} footer={<><Button variant="ghost" onClick={onClose}>Cancel</Button><Button variant="primary" onClick={submit}>Reassign</Button></>}>
      <div className="space-y-3">
        <label className="block">
          <div className="text-xs font-medium text-muted mb-1">Agent and model</div>
          <Select className="w-full" value={choice} onChange={(e) => setChoice(e.target.value)}>
            {options.map((o) => <option key={o.key} value={o.key}>{o.label}</option>)}
          </Select>
        </label>
        <label className="block">
          <div className="text-xs font-medium text-muted mb-1">Library agent (role)</div>
          <Select className="w-full" value={libAgent} onChange={(e) => setLibAgent(e.target.value)}>
            <option value="">None</option>
            {library.map((a) => <option key={a.id} value={a.id}>{a.display_name}</option>)}
          </Select>
        </label>
        {options.length === 0 && <Card className="p-3 text-xs text-muted">No agents available. Install one in Agents &amp; Models.</Card>}
      </div>
    </Dialog>
  );
}
