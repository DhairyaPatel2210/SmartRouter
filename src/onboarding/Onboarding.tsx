import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowRight, Bot, Check, Cloud, Cpu, Download, FlaskConical, FolderOpen, HardDrive, Laptop, Library, Loader2, Sparkles } from "lucide-react";
import { startPull, useApp } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { ScanResult, Suggestion } from "../lib/types";
import { Badge, Button, Card, Progress, Spinner, Switch } from "../components/ui";
import { FitBadge } from "../components/domain";
import { Logo, pickFolder } from "../components/Shell";
import { ModeCards } from "../home/Home";
import { ConnectFlow } from "../agents/CloudTab";
import { InstallDialog } from "../agents/InstallDialog";
import { bytes, cn, gb, pct, shortPath } from "../lib/format";

const STEPS = ["Scan", "Style", "Executor", "Workspace", "First run"];

export default function Onboarding() {
  const brand = useApp((s) => s.brand);
  const saveSettings = useApp((s) => s.saveSettings);
  const [step, setStep] = useState(0);
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [mode, setMode] = useState("balanced");
  const [budget, setBudget] = useState(false);
  const started = useMemo(() => Date.now(), []);

  useEffect(() => {
    api.scan().then((r) => {
      setScan(r);
      const paid = r.agents.some((a) => a.family === "paid" && a.installed && !a.id.startsWith("fake"));
      if (!paid) setBudget(true);
      useApp.setState({ agents: r.agents });
    }, () => setScan(null));
  }, []);

  const finish = async (goToRun?: string) => {
    await saveSettings({ onboarded: true, default_mode: mode, budget_planning: budget && !!useApp.getState().settings?.budget_planner });
    useApp.setState({ onboardingOpen: false });
    if (goToRun) useApp.getState().viewRun(goToRun);
    else useApp.getState().go("home");
    const secs = Math.round((Date.now() - started) / 1000);
    if (secs < 120) console.info(`onboarding finished in ${secs}s`);
  };
  const next = () => setStep((s) => Math.min(STEPS.length - 1, s + 1));

  return (
    <div className="h-full flex flex-col bg-bg">
      <div data-tauri-drag-region className="h-12 shrink-0 flex items-center justify-center gap-2">
        {STEPS.map((s, i) => (
          <div key={s} data-tauri-drag-region className="flex items-center gap-2">
            <span className={cn("h-1.5 rounded-full transition-all", i === step ? "w-6 bg-accent" : i < step ? "w-1.5 bg-accent/60" : "w-1.5 bg-line-strong")} />
          </div>
        ))}
      </div>
      <div className="flex-1 overflow-auto">
        <div key={step} className="max-w-[720px] mx-auto px-6 pb-10 pt-4 animate-fade-in">
          {step === 0 && <ScanStep scan={scan} brand={brand.productName} onNext={next} />}
          {step === 1 && <StyleStep mode={mode} setMode={setMode} budget={budget} setBudget={setBudget} scan={scan} onNext={next} />}
          {step === 2 && <ExecutorStep scan={scan} budget={budget} onNext={next} />}
          {step === 3 && <WorkspaceStep onNext={next} />}
          {step === 4 && <FirstRunStep mode={mode} onFinish={finish} />}
        </div>
      </div>
      <div className="h-14 shrink-0 flex items-center justify-between px-6 border-t border-line">
        <Button variant="ghost" onClick={() => (step === 0 ? finish() : setStep(step - 1))}>{step === 0 ? "Skip setup" : "Back"}</Button>
        <span className="text-xs text-faint">{STEPS[step]} · {step + 1} of {STEPS.length}</span>
        {step < STEPS.length - 1 ? <Button variant="ghost" onClick={next}>Skip <ArrowRight className="h-3.5 w-3.5" /></Button> : <Button variant="ghost" onClick={() => finish()}>Finish</Button>}
      </div>
    </div>
  );
}

function Title({ children, sub }: { children: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <div className="text-center mb-6">
      <h1 className="text-[22px] font-semibold tracking-tight">{children}</h1>
      {sub && <p className="text-muted mt-1.5 max-w-lg mx-auto leading-relaxed">{sub}</p>}
    </div>
  );
}

function ScanStep({ scan, brand, onNext }: { scan: ScanResult | null; brand: string; onNext: () => void }) {
  const agents = (scan?.agents ?? []).filter((a) => !a.id.startsWith("fake"));
  const cards = [
    { icon: Bot, label: "Paid agents", value: scan ? agents.filter((a) => a.family === "paid" && a.installed).map((a) => a.display_name).join(", ") || "none found" : null, ok: agents.some((a) => a.family === "paid" && a.installed) },
    { icon: Bot, label: "Open-source agents", value: scan ? agents.filter((a) => a.family === "open_source" && a.installed).map((a) => a.display_name).join(", ") || "none yet" : null, ok: agents.some((a) => a.family === "open_source" && a.installed) },
    { icon: HardDrive, label: "Local runtime", value: scan ? (scan.ollama.installed ? `Ollama${scan.ollama.running ? " (running)" : ""}` : "none") + (scan.runtimes.filter((r) => r.running).length ? ` + ${scan.runtimes.filter((r) => r.running).map((r) => r.kind).join(", ")}` : "") : null, ok: !!scan?.ollama.installed },
    { icon: Download, label: "Downloaded models", value: scan ? (scan.local_models.length ? scan.local_models.map((m) => m.name).join(", ") : "none") : null, ok: (scan?.local_models.length ?? 0) > 0 },
    { icon: Cpu, label: "This Mac", value: scan ? `${scan.hardware.hw.chip}, ${gb(scan.hardware.hw.total_mem_gb)}` : null, ok: true },
    { icon: Library, label: "Skills, agents & rules", value: scan ? (scan.library_found.length ? scan.library_found.join(", ") : "none found") : null, ok: (scan?.library_found.length ?? 0) > 0 },
  ];
  return (
    <>
      <div className="flex justify-center mb-4"><Logo className="h-14 w-14" /></div>
      <Title sub="Paid agents take the hard steps; open-source agents on local or cheap models do the rest. Let's see what you have.">Welcome to {brand}</Title>
      <div className="grid grid-cols-2 gap-3">
        {cards.map((c, i) => (
          <Card key={c.label} className={cn("p-4 transition-all duration-300", c.value != null && c.ok && "border-accent/40")} style={{ transitionDelay: `${i * 60}ms` }}>
            <div className="flex items-center gap-2 text-[12px] text-muted"><c.icon className={cn("h-4 w-4", c.value != null && c.ok ? "text-accent" : "text-faint")} />{c.label}</div>
            <div className="mt-1.5 font-medium text-[13px] min-h-5 truncate" title={c.value ?? ""}>{c.value ?? <Loader2 className="h-4 w-4 animate-spin text-faint" />}</div>
          </Card>
        ))}
      </div>
      <div className="flex items-center justify-center gap-3 mt-6">
        <Button variant="primary" size="lg" disabled={!scan} onClick={onNext}>Continue <ArrowRight className="h-4 w-4" /></Button>
        {scan && <span className="text-xs text-faint">Scanned in {(scan.elapsed_ms / 1000).toFixed(1)}s</span>}
      </div>
    </>
  );
}

function StyleStep({ mode, setMode, budget, setBudget, scan, onNext }: { mode: string; setMode: (m: string) => void; budget: boolean; setBudget: (b: boolean) => void; scan: ScanResult | null; onNext: () => void }) {
  const settings = useApp((s) => s.settings)!;
  const noPaid = !(scan?.agents ?? []).some((a) => a.family === "paid" && a.installed && !a.id.startsWith("fake"));
  return (
    <>
      <Title sub="You can switch per run, or mid-run, at any time.">Pick your style</Title>
      <ModeCards value={mode} onChange={setMode} modes={settings.modes} />
      {noPaid && (
        <Card className="p-4 mt-4 flex items-start gap-3">
          <Cloud className="h-5 w-5 text-cloud mt-0.5" />
          <div className="flex-1">
            <div className="font-medium">No paid agent found. Use budget planning?</div>
            <div className="text-[12.5px] text-muted mt-0.5">A strong, cheap cloud model plans and reviews instead. You'll pick it in the next step. You can add Cursor, Claude Code, Codex or Copilot later.</div>
          </div>
          <Switch checked={budget} onChange={setBudget} />
        </Card>
      )}
      <div className="flex justify-center mt-6"><Button variant="primary" size="lg" onClick={onNext}>Continue <ArrowRight className="h-4 w-4" /></Button></div>
    </>
  );
}

function ExecutorStep({ scan, budget, onNext }: { scan: ScanResult | null; budget: boolean; onNext: () => void }) {
  const settings = useApp((s) => s.settings)!;
  const agents = useApp((s) => s.agents);
  const pulls = useApp((s) => s.pulls);
  const catalog = useApp((s) => s.catalog);
  const os = useApp((s) => s.os);
  const setSettings = useApp((s) => s.setSettings);
  const toast = useApp((s) => s.toast);
  const best: Suggestion | undefined = scan?.suggestions.find((s) => s.best);
  const existing = scan?.local_models.find((m) => m.fit !== "wont_fit");
  const localPossible = !!existing || !!best;
  const [choice, setChoice] = useState<"local" | "cloud">(localPossible ? "local" : "cloud");
  const [status, setStatus] = useState<{ ok: boolean; message: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState<string | null>(null);
  const [phaseStart, setPhaseStart] = useState(0);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!phase) return;
    const t = setInterval(() => tick((x) => x + 1), 1000);
    return () => clearInterval(t);
  }, [phase]);
  const skipTest = useRef(false);
  const [install, setInstall] = useState<"opencode" | "ollama" | null>(null);
  const [folder, setFolder] = useState<string | null>(null);
  const opencode = agents.find((a) => a.id === "opencode");
  const ocInstalled = !!opencode?.installed;
  const target = existing?.name ?? best?.model.id;
  const pull = target ? pulls[target] : undefined;
  const pulling = pull && pull.done == null;

  useEffect(() => {
    if (pull?.done === true && target) void connectLocal(target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pull?.done]);

  const connectLocal = async (name: string) => {
    setBusy(true);
    skipTest.current = false;
    try {
      setPhase("Connecting the model to OpenCode…");
      setPhaseStart(Date.now());
      const st = await api.connectModel({ agent_id: "opencode", provider_id: "ollama", name, target: "pool_front" });
      setSettings(st);
      setPhase("Loading the model into memory for a quick test (the first load can take ~15 s)…");
      setPhaseStart(Date.now());
      const r = await api.smokeTest(st.executor_pool[0]);
      if (!skipTest.current) setStatus(r);
    } catch (e) {
      if (!skipTest.current) setStatus({ ok: false, message: errorText(e) });
    } finally {
      setBusy(false);
      setPhase(null);
    }
  };

  const goLocal = async () => {
    if (!target) return;
    if (existing) return connectLocal(existing.name);
    try {
      await startPull(target);
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const hasExecutor = settings.executor_pool.length > 0;
  const ollamaRt = catalog.runtimes.find((r) => r.id === "ollama");
  return (
    <>
      <Title sub={localPossible ? "Routine steps run on an open-source agent. Pick where its model lives." : "No local model fits this Mac comfortably, so a cheap cloud model is recommended."}>Choose your executor</Title>
      <div className="grid grid-cols-2 gap-3">
        <button onClick={() => setChoice("local")} className={cn("text-left rounded-xl border p-4", choice === "local" ? "border-accent ring-2 ring-accent/20 bg-accent-soft/30" : "border-line bg-panel")}>
          <div className="flex items-center gap-2 font-semibold"><Laptop className="h-4 w-4 text-local" /> Run locally</div>
          <div className="text-[12.5px] text-muted mt-1">OpenCode + {existing ? existing.catalog?.display_name ?? existing.name : best?.model.display_name ?? "a local model"}. Free, private, uses your Mac's memory.</div>
          {existing && <div className="mt-2"><Badge tone="ok">already downloaded: use existing</Badge></div>}
          {!existing && best && <div className="mt-2 flex gap-1.5"><FitBadge fit={best.fit} /><Badge>{gb(best.model.download_gb)} download</Badge></div>}
          {!localPossible && <div className="mt-2"><Badge tone="warn">no model fits</Badge></div>}
        </button>
        <button onClick={() => setChoice("cloud")} className={cn("text-left rounded-xl border p-4", choice === "cloud" ? "border-accent ring-2 ring-accent/20 bg-accent-soft/30" : "border-line bg-panel")}>
          <div className="flex items-center gap-2 font-semibold"><Cloud className="h-4 w-4 text-cloud" /> Use a cheap cloud model</div>
          <div className="text-[12.5px] text-muted mt-1">Paste one API key (OpenRouter, DeepSeek, Gemini…). Cents per task, keeps your Mac light.</div>
        </button>
      </div>

      <Card className="p-4 mt-4">
        {!ocInstalled && (
          <div className="flex items-center gap-3 mb-3 pb-3 border-b border-line">
            <Bot className="h-4 w-4 text-faint" />
            <span className="flex-1 text-[12.5px]">OpenCode (the open-source agent) isn't installed.</span>
            <Button size="sm" variant="primary" onClick={() => setInstall("opencode")}><Download className="h-3.5 w-3.5" /> Install OpenCode</Button>
          </div>
        )}
        {choice === "local" ? (
          <div className="space-y-3">
            {scan && !scan.ollama.installed && (
              <div className="flex items-center gap-3">
                <HardDrive className="h-4 w-4 text-faint" />
                <span className="flex-1 text-[12.5px]">Ollama runs local models. It isn't installed.</span>
                <Button size="sm" onClick={() => setInstall("ollama")}><Download className="h-3.5 w-3.5" /> Install Ollama</Button>
              </div>
            )}
            {target ? (
              <>
                <div className="text-[12.5px]">
                  {existing ? `Use ${existing.name}, already in ${shortPath(scan!.ollama.models_dir)}.` : `Download ${best!.model.display_name} (${gb(best!.model.download_gb)}) to `}
                  {!existing && <b className="font-mono text-[12px]">{shortPath(folder ?? scan!.ollama.models_dir)}</b>}
                  {!existing && (
                    <button className="ml-2 text-accent text-xs" onClick={async () => {
                      const p = await open({ directory: true, title: "Where should models go?" });
                      if (typeof p === "string") {
                        await api.setModelFolder(p);
                        setFolder(p);
                      }
                    }}>Change folder</button>
                  )}
                </div>
                {phase && (
                  <div className="flex items-center gap-2 text-[12.5px] text-muted">
                    <Spinner className="h-3.5 w-3.5" />
                    <span className="flex-1">{phase} <span className="text-faint">{Math.round((Date.now() - phaseStart) / 1000)}s</span></span>
                    {phase.startsWith("Loading") && (
                      <Button size="sm" variant="ghost" onClick={() => { skipTest.current = true; setPhase(null); setBusy(false); setStatus({ ok: true, message: "Connected. The test was skipped; the model loads on the first run." }); }}>Skip test</Button>
                    )}
                  </div>
                )}
                {pull?.error && !pulling && <div className="text-[12px] text-warn">{pull.error}</div>}
                {pulling && (
                  <div>
                    <Progress value={pull.total ? pct(pull.completed, pull.total) : 4} tone="local" className={pull.total ? "" : "animate-pulse-soft"} />
                    <div className="text-[11px] text-faint mt-1">{pull.status}{pull.total ? ` · ${bytes(pull.completed)} / ${bytes(pull.total)}` : ""}</div>
                  </div>
                )}
                {!status && !phase && <Button variant="primary" loading={busy || !!pulling} disabled={!ocInstalled || (!scan?.ollama.installed && !existing)} onClick={goLocal}>{existing ? "Connect & test" : "Download & connect"}</Button>}
              </>
            ) : (
              <div className="text-[12.5px] text-muted">No suggested model fits this Mac. Use a cheap cloud model instead.</div>
            )}
          </div>
        ) : (
          <ConnectFlow compact defaultTarget={budget ? "budget_planner" : "pool_back"} onDone={() => setStatus({ ok: true, message: "Cloud model connected." })} />
        )}
        {status && (
          <div className={cn("mt-3 text-[12.5px] flex items-center gap-2", status.ok ? "text-ok" : "text-warn")}>
            {status.ok ? <Check className="h-4 w-4" /> : null}{status.message}
          </div>
        )}
        {budget && choice === "cloud" && settings.budget_planner && <div className="mt-2 text-[12px] text-muted">Budget planner: {settings.budget_planner.model.display_name ?? settings.budget_planner.model.name}. Add a second model as executor later, or use this one for both.</div>}
      </Card>
      <div className="flex justify-center mt-6"><Button variant="primary" size="lg" onClick={onNext} disabled={!hasExecutor && !settings.budget_planner && !status?.ok}>Continue <ArrowRight className="h-4 w-4" /></Button></div>
      {install === "opencode" && <InstallDialog id="opencode" kind="agent" title="Install OpenCode" options={catalog.agents.find((a) => a.id === "opencode")?.install[os] ?? []} onClose={() => { setInstall(null); void useApp.getState().refreshAgents(true); }} />}
      {install === "ollama" && ollamaRt && <InstallDialog id="ollama" kind="runtime" title="Install Ollama" options={ollamaRt.install[os] ?? []} onClose={() => setInstall(null)} />}
    </>
  );
}

function WorkspaceStep({ onNext }: { onNext: () => void }) {
  const workspace = useApp((s) => s.workspace);
  const openFolder = useApp((s) => s.openFolder);
  const setWorkspace = useApp((s) => s.setWorkspace);
  const toast = useApp((s) => s.toast);
  const [imported, setImported] = useState<number | null>(null);
  return (
    <>
      <Title sub="Agents work inside the folder you choose. Not sure? Try the bundled sample project.">Pick a workspace</Title>
      <div className="grid grid-cols-2 gap-3">
        <button className="rounded-xl border border-line bg-panel p-5 text-left hover:border-accent" onClick={async () => { const p = await pickFolder(); if (p) await openFolder(p); }}>
          <FolderOpen className="h-5 w-5 text-accent" />
          <div className="font-semibold mt-2">Open a folder…</div>
          <div className="text-[12.5px] text-muted">Any project on your Mac.</div>
        </button>
        <button className="rounded-xl border border-line bg-panel p-5 text-left hover:border-accent" onClick={async () => { try { setWorkspace(await api.sampleProject()); } catch (e) { toast("error", errorText(e)); } }}>
          <FlaskConical className="h-5 w-5 text-local" />
          <div className="font-semibold mt-2">Sample project</div>
          <div className="text-[12.5px] text-muted">A tiny Node project with tests.</div>
        </button>
      </div>
      {workspace && (
        <Card className="p-4 mt-4 space-y-2">
          <div className="flex items-center gap-2"><Check className="h-4 w-4 text-ok" /><b>{workspace.row.display_name}</b><span className="text-xs text-faint font-mono truncate">{shortPath(workspace.row.path)}</span></div>
          {workspace.info.importable.length > 0 && imported === null && (
            <div className="flex items-center gap-3 text-[12.5px]">
              <span className="flex-1">Found {workspace.info.importable.join(", ")}. Import into the Library so every agent gets them?</span>
              <Button size="sm" onClick={async () => setImported(await api.libraryImport(workspace.row.id, "workspace"))}>Import</Button>
            </div>
          )}
          {imported !== null && <div className="text-[12.5px] text-ok">Imported {imported} item{imported === 1 ? "" : "s"}.</div>}
        </Card>
      )}
      <div className="flex justify-center mt-6"><Button variant="primary" size="lg" disabled={!workspace} onClick={onNext}>Continue <ArrowRight className="h-4 w-4" /></Button></div>
    </>
  );
}

function FirstRunStep({ mode, onFinish }: { mode: string; onFinish: (runId?: string) => void }) {
  const workspace = useApp((s) => s.workspace);
  const toast = useApp((s) => s.toast);
  const [goal, setGoal] = useState("Add a unit test to this project");
  const [busy, setBusy] = useState(false);
  return (
    <>
      <Title sub="Watch the planner hand off to the executor. The status bar shows memory in use the whole time.">Your first run</Title>
      <Card className="p-4">
        <textarea value={goal} onChange={(e) => setGoal(e.target.value)} rows={3} className="w-full bg-transparent outline-none resize-none text-[14px]" />
      </Card>
      <div className="flex justify-center gap-3 mt-6">
        <Button
          variant="primary"
          size="lg"
          loading={busy}
          disabled={!workspace || !goal.trim()}
          onClick={async () => {
            if (!workspace) return;
            setBusy(true);
            try {
              const id = await api.startRun({ workspace_id: workspace.row.id, goal: goal.trim(), mode, init_git: !workspace.info.is_git, dirty_strategy: workspace.info.dirty_count ? "stash" : null });
              onFinish(id);
            } catch (e) {
              toast("error", errorText(e));
              setBusy(false);
            }
          }}
        >
          <Sparkles className="h-4 w-4" /> Run it
        </Button>
        <Button size="lg" variant="ghost" onClick={() => onFinish()}>I'll do it later</Button>
      </div>
      {!workspace && <div className="text-center text-xs text-faint mt-3"><Spinner className="inline h-3 w-3" /> Pick a workspace first.</div>}
    </>
  );
}
