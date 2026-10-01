import { useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, Check, ExternalLink, KeyRound, Plug, Search, ShieldCheck, Trash2, Wrench } from "lucide-react";
import { useApp } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { CatalogProvider, CloudModel, ProviderType, ProviderView } from "../lib/types";
import { Badge, Button, Card, Dialog, InlineEdit, Input, SectionTitle, Select, Spinner, Switch } from "../components/ui";
import { cn, usd } from "../lib/format";

export default function CloudTab() {
  const toast = useApp((s) => s.toast);
  const [providers, setProviders] = useState<ProviderView[] | null>(null);
  const [connect, setConnect] = useState(false);
  const [browse, setBrowse] = useState<ProviderView | null>(null);
  const reload = () => api.providersList().then(setProviders, () => setProviders([]));
  useEffect(() => {
    void reload();
  }, []);
  return (
    <div className="space-y-6">
      <Card className="p-4 flex items-center gap-4">
        <ShieldCheck className="h-5 w-5 text-ok shrink-0" />
        <div className="flex-1 text-[12.5px] text-muted leading-relaxed">
          Keys live in the macOS Keychain and are shown only as their last 4 characters. Agents get them through an environment variable at run time; they're never written to files, logs or the database.
        </div>
        <Button variant="primary" onClick={() => setConnect(true)}><Plug className="h-3.5 w-3.5" /> Connect a provider</Button>
      </Card>
      <div>
        <SectionTitle>Connected providers</SectionTitle>
        <Card className="divide-y divide-line">
          {providers === null && <div className="p-4"><Spinner /></div>}
          {providers?.length === 0 && <div className="p-4 text-[12.5px] text-muted">None yet. A cheap cloud model lets you run without a local model; DeepSeek, Gemini Flash and OpenRouter models cost cents per task.</div>}
          {providers?.map((p) => (
            <div key={p.id} className="flex items-center gap-3 px-4 py-3">
              <div className="flex-1 min-w-0">
                <InlineEdit value={p.display_name} className="font-medium" onSave={async (v) => { await api.updateProvider({ ...p, display_name: v }); void reload(); }} />
                <div className="text-[11.5px] text-muted font-mono truncate">{p.base_url}</div>
              </div>
              {p.key_hint ? <Badge title="Stored in the Keychain"><KeyRound className="h-3 w-3" />{p.key_hint}</Badge> : <Badge>no key</Badge>}
              <label className="flex items-center gap-1.5 text-[11.5px] text-muted" title="Pause this provider when its estimated spend this month reaches the cap">
                cap $
                <input
                  className="w-14 h-7 rounded-md bg-panel border border-line px-1.5 text-xs"
                  type="number"
                  min={0}
                  defaultValue={p.monthly_cap_usd ?? ""}
                  placeholder="none"
                  onBlur={async (e) => { const v = e.target.value === "" ? null : Number(e.target.value); await api.updateProvider({ ...p, monthly_cap_usd: v }); }}
                />
                /mo
              </label>
              <Switch checked={p.enabled} onChange={async (v) => { await api.updateProvider({ ...p, enabled: v }); void reload(); }} />
              <Button size="sm" onClick={() => setBrowse(p)}>Models</Button>
              <Button size="icon" variant="ghost" className="hover:text-bad" title="Disconnect and delete the key" onClick={async () => {
                await api.disconnectProvider(p.id);
                toast("info", `Disconnected ${p.display_name}; its key was removed from the Keychain.`);
                void reload();
              }}><Trash2 className="h-3.5 w-3.5" /></Button>
            </div>
          ))}
        </Card>
      </div>
      {connect && <Dialog open onClose={() => { setConnect(false); void reload(); }} width={680} title="Connect a cheap cloud provider"><ConnectFlow onDone={() => { setConnect(false); void reload(); }} /></Dialog>}
      {browse && (
        <Dialog open onClose={() => setBrowse(null)} width={680} title={`${browse.display_name} models`}>
          <ConnectFlow startWith={browse} onDone={() => setBrowse(null)} />
        </Dialog>
      )}
    </div>
  );
}

type Step = "pick" | "key" | "models";

/** Pick provider → paste key (tested, stored in Keychain) → pick a model → connect to an agent + smoke test. */
export function ConnectFlow({ onDone, startWith, defaultTarget = "pool_back", compact }: { onDone: () => void; startWith?: ProviderView; defaultTarget?: "pool_front" | "pool_back" | "budget_planner"; compact?: boolean }) {
  const catalog = useApp((s) => s.catalog);
  const agents = useApp((s) => s.agents);
  const setSettings = useApp((s) => s.setSettings);
  const toast = useApp((s) => s.toast);
  const [step, setStep] = useState<Step>(startWith ? "models" : "pick");
  const [prov, setProv] = useState<CatalogProvider | null>(
    startWith ? { id: startWith.id, display_name: startWith.display_name, type: startWith.type, base_url: startWith.base_url ?? "", signup_url: "", key_prefix: "", notes: "" } : null,
  );
  const [key, setKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [customName, setCustomName] = useState("");
  const [models, setModels] = useState<CloudModel[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [toolsOnly, setToolsOnly] = useState(true);
  const [picked, setPicked] = useState<CloudModel | null>(null);
  const [target, setTarget] = useState(defaultTarget);
  const oss = agents.filter((a) => a.family === "open_source" && a.installed && a.enabled);
  const [agent, setAgent] = useState<string>(oss.find((a) => a.id === "opencode")?.id ?? oss[0]?.id ?? "opencode");

  useEffect(() => {
    if (startWith) api.providerModels(startWith.id).then(setModels, (e) => setErr(errorText(e)));
  }, [startWith]);

  const submitKey = async () => {
    if (!prov) return;
    setBusy(true);
    setErr(null);
    try {
      const isCustom = prov.id === "custom";
      const ms = await api.connectProvider({
        id: isCustom ? customName || "custom" : prov.id,
        display_name: isCustom ? customName || "Custom provider" : prov.display_name,
        type: prov.type as ProviderType,
        base_url: isCustom ? baseUrl : prov.base_url,
        key: key || null,
      });
      if (isCustom) setProv({ ...prov, id: (customName || "custom").toLowerCase().replace(/[^a-z0-9]+/g, "-") });
      setModels(ms);
      setStep("models");
    } catch (e) {
      setErr(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const shown = useMemo(() => {
    const list = (models ?? []).filter((m) => (!toolsOnly || m.tool_calling !== false) && (m.name + m.id).toLowerCase().includes(q.toLowerCase()));
    return list.sort((a, b) => (a.price_in_per_m ?? 99) - (b.price_in_per_m ?? 99)).slice(0, 300);
  }, [models, q, toolsOnly]);

  const finish = async () => {
    if (!picked || !prov) return;
    setBusy(true);
    try {
      const st = await api.connectModel({
        agent_id: agent,
        provider_id: prov.id,
        name: picked.id,
        display_name: picked.name !== picked.id ? picked.name : null,
        price_in_per_m: picked.price_in_per_m,
        price_out_per_m: picked.price_out_per_m,
        ctx_len: picked.ctx_len,
        tool_calling: picked.tool_calling,
        target,
      });
      setSettings(st);
      const entry = target === "budget_planner" ? st.budget_planner! : target === "pool_front" ? st.executor_pool[0] : st.executor_pool[st.executor_pool.length - 1];
      const r = await api.smokeTest(entry);
      toast(r.ok ? "info" : "warn", r.ok ? `Connected. Smoke test: ${r.message}` : `Connected, but the smoke test failed: ${r.message}`);
      onDone();
    } catch (e) {
      setErr(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3 pb-2">
      {step === "pick" && (
        <div className={cn("grid gap-2", compact ? "grid-cols-2" : "grid-cols-3")}>
          {catalog.providers.map((p) => (
            <button key={p.id} onClick={() => { setProv(p); setStep("key"); setBaseUrl(p.base_url); }} className="text-left rounded-xl border border-line p-3 hover:border-accent hover:bg-accent-soft/20">
              <div className="font-medium">{p.display_name}</div>
              <div className="text-[11.5px] text-muted mt-0.5 leading-snug">{p.notes}</div>
            </button>
          ))}
        </div>
      )}
      {step === "key" && prov && (
        <div className="space-y-3">
          <button className="text-xs text-muted flex items-center gap-1 hover:text-fg" onClick={() => setStep("pick")}><ArrowLeft className="h-3 w-3" /> All providers</button>
          {prov.id === "custom" && (
            <div className="grid grid-cols-2 gap-2">
              <Input placeholder="Name (e.g. Together)" value={customName} onChange={(e) => setCustomName(e.target.value)} />
              <Input placeholder="Base URL (https://…/v1)" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
            </div>
          )}
          <div className="flex gap-2">
            <Input autoFocus type="password" placeholder={`${prov.display_name} API key${prov.key_prefix ? ` (${prov.key_prefix}…)` : ""} · empty reuses a saved key`} value={key} onChange={(e) => setKey(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submitKey()} />
            <Button variant="primary" loading={busy} onClick={submitKey}>Test & save</Button>
          </div>
          {prov.signup_url && <button className="text-xs text-accent flex items-center gap-1" onClick={() => openUrl(prov.signup_url)}>Get a key from {prov.display_name} <ExternalLink className="h-3 w-3" /></button>}
        </div>
      )}
      {step === "models" && (
        <div className="space-y-3">
          <div className="flex items-center gap-2">
            <div className="relative flex-1">
              <Search className="h-3.5 w-3.5 absolute left-2.5 top-2.5 text-faint" />
              <Input className="pl-8" placeholder="Search models" value={q} onChange={(e) => setQ(e.target.value)} />
            </div>
            <label className="flex items-center gap-2 text-[12px] text-muted whitespace-nowrap"><Wrench className="h-3.5 w-3.5" /> Tool calling only <Switch checked={toolsOnly} onChange={setToolsOnly} /></label>
          </div>
          <div className="h-64 overflow-auto rounded-lg border border-line divide-y divide-line">
            {models === null && <div className="p-4"><Spinner /></div>}
            {models?.length === 0 && <div className="p-4 text-muted text-[12.5px]">The provider returned no models.</div>}
            {shown.map((m) => (
              <button key={m.id} onClick={() => setPicked(m)} className={cn("w-full flex items-center gap-3 px-3 py-2 text-left", picked?.id === m.id ? "bg-accent-soft/50" : "hover:bg-panel-2")}>
                <span className="flex-1 min-w-0">
                  <span className="block truncate font-medium text-[12.5px]">{m.name}</span>
                  {m.name !== m.id && <span className="block truncate text-[11px] text-faint font-mono">{m.id}</span>}
                </span>
                {m.tool_calling === true && <Badge tone="ok">tools</Badge>}
                {m.tool_calling === false && <Badge tone="warn">no tools</Badge>}
                {m.ctx_len && <span className="text-[11px] text-faint w-12 text-right">{Math.round(m.ctx_len / 1000)}k</span>}
                <span className="text-[11px] text-muted w-28 text-right">{m.price_in_per_m != null ? `${usd(m.price_in_per_m, 2)} / ${usd(m.price_out_per_m, 2)} per M` : "price n/a"}</span>
                {picked?.id === m.id && <Check className="h-4 w-4 text-accent" />}
              </button>
            ))}
          </div>
          <div className="flex items-center gap-2 flex-wrap text-[12.5px]">
            <span className="text-muted">Run it with</span>
            <Select value={agent} onChange={(e) => setAgent(e.target.value)}>
              {(oss.length ? oss : agents.filter((a) => a.id === "opencode")).map((a) => <option key={a.id} value={a.id}>{a.display_name}{a.installed ? "" : " (not installed)"}</option>)}
            </Select>
            <span className="text-muted">as</span>
            <Select value={target} onChange={(e) => setTarget(e.target.value as typeof target)}>
              <option value="pool_back">a cheap executor (end of pool)</option>
              <option value="pool_front">the first-choice executor</option>
              <option value="budget_planner">the budget planner & reviewer</option>
            </Select>
            <Button variant="primary" className="ml-auto" disabled={!picked} loading={busy} onClick={finish}>Connect & test</Button>
          </div>
          {picked?.tool_calling === false && <div className="text-[12px] text-warn">This model doesn't advertise tool calling; coding agents need tools to edit files.</div>}
        </div>
      )}
      {err && <div className="text-[12.5px] text-bad selectable">{err}</div>}
    </div>
  );
}
