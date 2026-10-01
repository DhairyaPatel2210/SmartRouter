import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Check, Cpu, Download, FileUp, FolderOpen, HardDrive, Pause, Play, Plus, Power, Square, Trash2, X } from "lucide-react";
import { startPull, useApp } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { DetectedRuntime, HardwareCheck, LocalModel, OllamaStatus, Suggestion } from "../lib/types";
import { Badge, Button, Card, Dialog, Input, Progress, SectionTitle, Spinner, Switch } from "../components/ui";
import { FitBadge } from "../components/domain";
import { InstallDialog } from "./InstallDialog";
import { bytes, gb, pct, shortPath } from "../lib/format";

export default function LocalTab() {
  const toast = useApp((s) => s.toast);
  const pulls = useApp((s) => s.pulls);
  const move = useApp((s) => s.move);
  const catalog = useApp((s) => s.catalog);
  const os = useApp((s) => s.os);
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  const [hw, setHw] = useState<HardwareCheck | null>(null);
  const [models, setModels] = useState<LocalModel[] | null>(null);
  const [sugg, setSugg] = useState<Suggestion[]>([]);
  const [runtimes, setRuntimes] = useState<DetectedRuntime[]>([]);
  const [showAll, setShowAll] = useState(false);
  const [installRt, setInstallRt] = useState(false);
  const [gguf, setGguf] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const reload = useCallback(() => {
    api.ollamaStatus().then(setStatus, () => {});
    api.hardwareCheck().then(setHw, () => {});
    api.localModels().then(setModels, () => setModels([]));
    api.modelSuggestions().then(setSugg, () => {});
    api.detectRuntimes().then(setRuntimes, () => {});
  }, []);
  useEffect(reload, [reload]);
  const doneCount = Object.values(pulls).filter((p) => p.done === true).length;
  useEffect(() => {
    if (doneCount) reload();
  }, [doneCount, reload]);
  useEffect(() => {
    if (move?.done != null) reload();
  }, [move?.done, reload]);

  const act = async (key: string, f: () => Promise<unknown>, ok?: string) => {
    setBusy(key);
    try {
      await f();
      if (ok) toast("info", ok);
      reload();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const ollama = catalog.runtimes.find((r) => r.id === "ollama");
  const visible = sugg.filter((s) => showAll || s.fit !== "wont_fit");
  const moving = move && move.done == null;

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-3 gap-3">
        <Card className="p-4">
          <div className="flex items-center gap-2 font-medium"><Power className="h-4 w-4 text-faint" /> Ollama</div>
          {!status ? <Spinner className="mt-3" /> : (
            <>
              <div className="flex gap-1.5 mt-2 flex-wrap">
                {status.installed ? <Badge tone="ok">installed</Badge> : <Badge tone="warn">not installed</Badge>}
                {status.running ? <Badge tone="ok">running{status.version ? ` · ${status.version}` : ""}</Badge> : status.installed && <Badge>stopped</Badge>}
                {status.managed_by_app && <Badge tone="accent" title="Started by this app; stops when no longer needed">managed</Badge>}
              </div>
              <div className="flex gap-2 mt-3">
                {!status.installed && ollama && <Button size="sm" variant="primary" onClick={() => setInstallRt(true)}><Download className="h-3.5 w-3.5" /> Install</Button>}
                {status.installed && !status.running && <Button size="sm" loading={busy === "start"} onClick={() => act("start", api.ollamaStart, "Ollama started.")}><Play className="h-3.5 w-3.5" /> Start</Button>}
                {status.managed_by_app && <Button size="sm" loading={busy === "stop"} onClick={() => act("stop", api.ollamaStop, "Ollama stopped.")}><Square className="h-3.5 w-3.5" /> Stop</Button>}
              </div>
            </>
          )}
        </Card>
        <Card className="p-4">
          <div className="flex items-center gap-2 font-medium"><Cpu className="h-4 w-4 text-faint" /> This Mac</div>
          {hw && (
            <div className="text-[12.5px] mt-2 space-y-0.5">
              <div>{hw.hw.chip} · {gb(hw.hw.total_mem_gb)}</div>
              <div className="text-muted">Local-model budget: <b className="text-fg">{gb(hw.budget_gb)}</b> ({hw.budget_pct}%)</div>
              <div className="text-muted">{gb(hw.free_mem_gb)} free right now</div>
            </div>
          )}
        </Card>
        <Card className="p-4">
          <div className="flex items-center gap-2 font-medium"><HardDrive className="h-4 w-4 text-faint" /> Model folder</div>
          {status && (
            <>
              <div className="text-[12px] font-mono mt-2 truncate selectable" title={status.models_dir}>{shortPath(status.models_dir)}</div>
              {hw?.free_disk_gb != null && <div className="text-[12px] text-muted">{gb(hw.free_disk_gb)} free on this disk</div>}
              <div className="flex gap-1.5 mt-2 flex-wrap">
                <Button size="sm" onClick={async () => {
                  const p = await open({ directory: true, title: "Choose where models are stored" });
                  if (typeof p === "string") await act("folder", () => api.setModelFolder(p), "Model folder changed. New downloads go there.");
                }}>Change…</Button>
                <Button size="sm" disabled={!!moving || !models?.length} onClick={async () => {
                  const p = await open({ directory: true, title: "Move all models to…" });
                  if (typeof p === "string") await act("move", () => api.moveModels(p));
                }}>Move models…</Button>
                <Button size="icon" variant="ghost" title="Show in Finder" onClick={() => revealItemInDir(status.models_dir).catch(() => {})}><FolderOpen className="h-3.5 w-3.5" /></Button>
              </div>
            </>
          )}
        </Card>
      </div>

      {status?.folder_hint && <Card className="p-3 text-[12.5px] bg-warn-soft/50 border-warn/30 selectable">{status.folder_hint}</Card>}
      {move && (
        <Card className="p-3">
          <div className="flex items-center justify-between text-[12.5px] mb-2">
            <span>{move.done == null ? "Moving models…" : move.done ? "Models moved." : `Move failed: ${move.error}`}</span>
            {move.done == null && <Button size="sm" variant="ghost" onClick={() => api.cancelJob("move-models")}>Cancel</Button>}
            {move.done != null && <Button size="icon" variant="ghost" onClick={() => useApp.setState({ move: null })}><X className="h-3.5 w-3.5" /></Button>}
          </div>
          {move.done == null && <><Progress value={pct(move.completed, move.total)} /><div className="text-[11px] text-faint mt-1">{bytes(move.completed)} of {bytes(move.total)}</div></>}
        </Card>
      )}

      <div>
        <SectionTitle right={<Button size="sm" variant="ghost" onClick={async () => {
          const p = await open({ filters: [{ name: "GGUF", extensions: ["gguf"] }], title: "Import a GGUF model" });
          if (typeof p === "string") setGguf(p);
        }}><FileUp className="h-3.5 w-3.5" /> Import GGUF</Button>}>On this Mac</SectionTitle>
        <Card className="divide-y divide-line">
          {models === null && <div className="p-4"><Spinner /></div>}
          {models?.length === 0 && <div className="p-4 text-[12.5px] text-muted">No local models yet. Download a suggested one below, or use a cheap cloud model instead.</div>}
          {models?.map((m) => <InstalledRow key={m.name} m={m} busy={busy} act={act} />)}
        </Card>
        <p className="text-[11.5px] text-faint mt-2">Models already in your Ollama folder are reused; nothing is downloaded twice.</p>
      </div>

      <div>
        <SectionTitle right={<label className="flex items-center gap-2 text-[12px] text-muted">Show models that won't fit <Switch checked={showAll} onChange={setShowAll} /></label>}>Suggested for this Mac</SectionTitle>
        <div className="grid grid-cols-2 gap-3">
          {visible.map((s) => <SuggestionCard key={s.model.id} s={s} pull={pulls[s.model.id]} />)}
        </div>
      </div>

      {runtimes.some((r) => r.running) && (
        <div>
          <SectionTitle>Other local runtimes</SectionTitle>
          <Card className="divide-y divide-line">
            {runtimes.filter((r) => r.running).map((r) => <RuntimeRow key={r.endpoint} r={r} />)}
          </Card>
        </div>
      )}

      {installRt && ollama && <InstallDialog id="ollama" kind="runtime" title="Install Ollama" options={ollama.install[os] ?? []} onClose={() => { setInstallRt(false); reload(); }} />}
      {gguf && <ImportGguf path={gguf} onClose={() => { setGguf(null); reload(); }} />}
    </div>
  );
}

/** Connects a local model to the first open-source agent that can drive it, then smoke-tests it. */
export function useConnectLocal() {
  const agents = useApp((s) => s.agents);
  const setSettings = useApp((s) => s.setSettings);
  const toast = useApp((s) => s.toast);
  return async (name: string, providerId = "ollama") => {
    const agent = agents.find((a) => a.family === "open_source" && a.installed && a.enabled && a.providers.includes(providerId === "ollama" ? "ollama" : (providerId as never)))
      ?? agents.find((a) => a.id === "opencode");
    if (!agent) throw new Error("Install an open-source agent (OpenCode) first.");
    const st = await api.connectModel({ agent_id: agent.id, provider_id: providerId, name, target: "pool_front" });
    setSettings(st);
    const entry = st.executor_pool[0];
    const r = await api.smokeTest(entry);
    toast(r.ok ? "info" : "warn", r.ok ? `Connected: ${r.message}` : `Connected, but the smoke test failed: ${r.message}`);
  };
}

function InstalledRow({ m, busy, act }: { m: LocalModel; busy: string | null; act: (k: string, f: () => Promise<unknown>, ok?: string) => Promise<void> }) {
  const connect = useConnectLocal();
  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="font-medium flex items-center gap-2">
          {m.catalog?.display_name ?? m.name}
          {m.loaded && <Badge tone="info">loaded</Badge>}
          {m.in_pool && <Badge tone="local"><Check className="h-3 w-3" /> in pool</Badge>}
          {m.catalog?.agentic === false && <Badge tone="warn" title="Too small to call tools reliably inside coding agents; steps on it will likely fail and escalate">chores only</Badge>}
        </div>
        <div className="text-[11.5px] text-muted font-mono">{m.name} · {gb(m.size_gb)} on disk · ~{gb(m.mem_gb)} RAM at {Math.round(m.ctx / 1024)}k context</div>
      </div>
      <FitBadge fit={m.fit} />
      {m.loaded && <Button size="sm" variant="ghost" loading={busy === `unload:${m.name}`} onClick={() => act(`unload:${m.name}`, () => api.unloadModel(m.name), "Unloaded.")}>Unload</Button>}
      {!m.in_pool && <Button size="sm" loading={busy === `use:${m.name}`} onClick={() => act(`use:${m.name}`, () => connect(m.name))}><Plus className="h-3.5 w-3.5" /> Use</Button>}
      <Button size="icon" variant="ghost" className="hover:text-bad" title="Remove from disk" loading={busy === `del:${m.name}`} onClick={() => act(`del:${m.name}`, async () => { const g = await api.deleteModel(m.name); return g; }, `Removed ${m.name} (${gb(m.size_gb)} freed).`)}>
        <Trash2 className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}

function SuggestionCard({ s, pull }: { s: Suggestion; pull?: { status: string; completed: number; total: number; done: boolean | null; error: string | null } }) {
  const toast = useApp((st) => st.toast);
  const connect = useConnectLocal();
  const active = pull && pull.done == null;
  return (
    <Card className={`p-4 ${s.best ? "border-accent/50 ring-1 ring-accent/20" : ""}`}>
      <div className="flex items-start gap-2">
        <div className="flex-1 min-w-0">
          <div className="font-semibold flex items-center gap-2">
            {s.model.display_name}
            {s.best && <Badge tone="accent">best fit</Badge>}
            {s.model.agentic === false && <Badge tone="warn">chores only</Badge>}
          </div>
          <div className="text-[12px] text-muted mt-0.5">{s.model.good_for}</div>
        </div>
        <FitBadge fit={s.fit} />
      </div>
      <div className="text-[11.5px] text-faint mt-2">{gb(s.model.download_gb)} download · ~{gb(s.mem_gb)} RAM · {Math.round(s.ctx / 1024)}k context · {s.model.quant}</div>
      {active && (
        <div className="mt-3">
          <Progress value={pull.total ? pct(pull.completed, pull.total) : 4} tone="local" className={pull.total ? "" : "animate-pulse-soft"} />
          <div className="flex items-center justify-between mt-1 text-[11px] text-faint">
            <span>{pull.status}{pull.total ? ` · ${bytes(pull.completed)} / ${bytes(pull.total)}` : ""}</span>
            <button className="flex items-center gap-1 hover:text-fg" onClick={() => api.cancelPull(s.model.id)}><Pause className="h-3 w-3" /> Pause</button>
          </div>
        </div>
      )}
      {pull?.error && !active && <div className="text-[11.5px] text-warn mt-2">{pull.error}</div>}
      <div className="flex gap-2 mt-3">
        {s.installed ? (
          <Badge tone="ok"><Check className="h-3 w-3" /> downloaded</Badge>
        ) : (
          !active && (
            <Button size="sm" variant={s.best ? "primary" : "secondary"} disabled={!s.disk_ok} title={s.disk_ok ? undefined : "Not enough disk space in the model folder"} onClick={() => startPull(s.model.id)}>
              <Download className="h-3.5 w-3.5" /> {pull?.error ? "Resume" : "Download"}
            </Button>
          )
        )}
        {s.installed && <Button size="sm" onClick={() => connect(s.model.id).catch((e) => toast("error", errorText(e)))}><Plus className="h-3.5 w-3.5" /> Use</Button>}
      </div>
    </Card>
  );
}

function RuntimeRow({ r }: { r: DetectedRuntime }) {
  const toast = useApp((s) => s.toast);
  const connect = useConnectLocal();
  const name = { lmstudio: "LM Studio", llamacpp: "llama.cpp server", mlx: "MLX server" }[r.kind as string] ?? r.kind;
  const [model, setModel] = useState(r.models[0] ?? "");
  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <div className="flex-1">
        <div className="font-medium">{name} <Badge tone="ok">running</Badge></div>
        <div className="text-[11.5px] text-muted font-mono">{r.endpoint}</div>
      </div>
      {r.models.length > 0 && (
        <select className="h-7 rounded-md bg-panel border border-line text-xs px-2" value={model} onChange={(e) => setModel(e.target.value)}>
          {r.models.map((m) => <option key={m}>{m}</option>)}
        </select>
      )}
      <Button size="sm" disabled={!model} onClick={async () => {
        try {
          await api.connectProvider({ id: r.kind, display_name: name, type: r.kind, base_url: r.endpoint });
          await connect(model, r.kind);
        } catch (e) {
          toast("error", errorText(e));
        }
      }}><Plus className="h-3.5 w-3.5" /> Use</Button>
    </div>
  );
}

function ImportGguf({ path, onClose }: { path: string; onClose: () => void }) {
  const toast = useApp((s) => s.toast);
  const base = path.split("/").pop()!.replace(/\.gguf$/i, "").toLowerCase().replace(/[^a-z0-9._-]+/g, "-");
  const [name, setName] = useState(base);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog open onClose={onClose} title="Import GGUF into Ollama" footer={<><Button variant="ghost" onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} onClick={async () => {
      setBusy(true);
      try {
        await api.importGguf(path, name);
        toast("info", `Imported ${name}.`);
        onClose();
      } catch (e) {
        toast("error", errorText(e));
      } finally {
        setBusy(false);
      }
    }}>Import</Button></>}>
      <div className="space-y-2 text-[12.5px]">
        <div className="font-mono text-muted truncate">{shortPath(path)}</div>
        <label className="block">Model name<Input className="mt-1" value={name} onChange={(e) => setName(e.target.value)} /></label>
      </div>
    </Dialog>
  );
}
