// Visual workflow builder (M5). React Flow is loaded only with this screen.
// Workflows save as JSON in .orchestrator/workflows/ and compile into a
// routing mode the engine runs (see docs/DECISIONS.md).
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  addEdge,
  Background,
  Controls,
  Handle,
  MiniMap,
  Position,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  type Connection,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { CheckCircle2, Copy, GitFork, Hand, ListChecks, Play, Plus, Save, ScanEye, Sparkles, Trash2, Wand2 } from "lucide-react";
import { useApp } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { StepClass } from "../lib/types";
import { compileWorkflow, templateFromMode, type NodeData, type NodeKind, type Workflow } from "./model";
import { Badge, Button, Card, InlineEdit, Select } from "../components/ui";
import { ScreenHeader } from "../components/Shell";
import { cn } from "../lib/format";

const KIND_META: Record<NodeKind, { label: string; icon: typeof Play; color: string }> = {
  plan: { label: "Plan", icon: ListChecks, color: "var(--premium)" },
  execute: { label: "Execute", icon: Play, color: "var(--local)" },
  verify: { label: "Verify", icon: CheckCircle2, color: "var(--ok)" },
  review: { label: "Review", icon: ScanEye, color: "var(--premium)" },
  condition: { label: "Condition", icon: GitFork, color: "var(--warn)" },
  approval: { label: "Human approval", icon: Hand, color: "var(--cloud)" },
};

function StepNode({ data, selected }: NodeProps<Node<NodeData>>) {
  const meta = KIND_META[data.kind];
  const agents = useApp((s) => s.agents);
  const agent = data.agent === "router" ? "Router decides" : agents.find((a) => a.id === data.agent)?.display_name ?? data.agent;
  return (
    <div className={cn("rounded-xl border bg-elev px-3 py-2 min-w-[170px] shadow-sm", selected ? "border-accent ring-2 ring-accent/20" : "border-line")}>
      <Handle type="target" position={Position.Left} className="!bg-line-strong !w-2 !h-2" />
      <div className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider" style={{ color: meta.color }}>
        <meta.icon className="h-3.5 w-3.5" />
        {meta.label}
      </div>
      <div className="text-[12.5px] font-medium mt-0.5">{data.label}</div>
      {(data.kind === "execute" || data.kind === "plan" || data.kind === "review") && (
        <div className="text-[11px] text-muted mt-0.5">{agent}{data.tier !== "any" ? ` · ${data.tier === "premium" ? "paid" : data.tier === "local" ? "local" : "cheap"}` : ""}{data.libraryAgent ? ` · as ${data.libraryAgent}` : ""}</div>
      )}
      <Handle type="source" position={Position.Right} className="!bg-line-strong !w-2 !h-2" />
    </div>
  );
}

const nodeTypes = { step: StepNode };

export default function Workflows() {
  return (
    <ReactFlowProvider>
      <Builder />
    </ReactFlowProvider>
  );
}

function Builder() {
  const settings = useApp((s) => s.settings)!;
  const saveSettings = useApp((s) => s.saveSettings);
  const workspace = useApp((s) => s.workspace);
  const toast = useApp((s) => s.toast);
  const wsId = workspace?.row.id ?? null;
  const templates = useMemo(() => settings.modes.filter((m) => m.builtin).map(templateFromMode), [settings.modes]);
  const [saved, setSaved] = useState<Workflow[]>([]);
  const [current, setCurrent] = useState<Workflow>(templates[1] ?? templates[0]);
  const [nodes, setNodes, onNodesChange] = useNodesState<Node<NodeData>>(current.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>(current.edges);
  const [sel, setSel] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);

  const reload = useCallback(() => {
    api.workflowList(wsId).then((l) => setSaved(l as unknown as Workflow[]), () => {});
  }, [wsId]);
  useEffect(reload, [reload]);

  const load = (w: Workflow) => {
    setCurrent(w);
    setNodes(w.nodes);
    setEdges(w.edges);
    setSel(null);
    setDirty(false);
  };

  const onConnect = useCallback((c: Connection) => {
    setEdges((es) => addEdge({ ...c, id: `e-${Date.now()}` }, es));
    setDirty(true);
  }, [setEdges]);

  const addNode = (kind: NodeKind) => {
    const id = `${kind}-${Math.random().toString(36).slice(2, 7)}`;
    setNodes((ns) => [...ns, { id, type: "step", position: { x: 120 + ns.length * 30, y: 340 }, data: { kind, label: KIND_META[kind].label, agent: "router", tier: kind === "execute" ? "cheap_cloud" : "any", classes: kind === "execute" ? [] : undefined } }]);
    setSel(id);
    setDirty(true);
  };

  const editable = !current.builtin;
  const persist = async (w: Workflow) => {
    try {
      await api.workflowSave(wsId, w as unknown as Record<string, unknown>);
      setCurrent(w);
      setDirty(false);
      reload();
      toast("info", `Saved to ${wsId ? `${useApp.getState().brand.handoffDirName}/workflows` : "the global workflows folder"}.`);
    } catch (e) {
      toast("error", errorText(e));
    }
  };
  const saveCurrent = () => persist({ ...current, nodes, edges });
  const copyAsNew = () => {
    const id = `${current.id.replace(/^template-/, "")}-${Math.random().toString(36).slice(2, 6)}`;
    void persist({ id, display_name: `${current.display_name} (custom)`, nodes, edges });
  };
  const useAsMode = async () => {
    const mode = compileWorkflow({ ...current, nodes, edges });
    const modes = [...settings.modes.filter((m) => m.id !== mode.id), mode];
    await saveSettings({ modes });
    toast("info", `"${mode.display_name}" is now a mode: pick it on Home or as the default in Settings.`);
  };

  const selNode = nodes.find((n) => n.id === sel);
  const selEdge = edges.find((e) => e.id === sel);
  const update = (patch: Partial<NodeData>) => {
    setNodes((ns) => ns.map((n) => (n.id === sel ? { ...n, data: { ...n.data, ...patch } } : n)));
    setDirty(true);
  };
  const compiled = compileWorkflow({ ...current, nodes, edges });

  return (
    <div className="flex flex-col h-full">
      <ScreenHeader title="Workflows" subtitle="Design how steps flow between agents. Saved workflows become modes you can run.">
        {editable ? (
          <Button className="no-drag" variant={dirty ? "primary" : "secondary"} onClick={saveCurrent}><Save className="h-3.5 w-3.5" /> Save</Button>
        ) : (
          <Button className="no-drag" onClick={copyAsNew}><Copy className="h-3.5 w-3.5" /> Copy to edit</Button>
        )}
        <Button className="no-drag" variant="primary" onClick={useAsMode} disabled={current.builtin}><Sparkles className="h-3.5 w-3.5" /> Use as mode</Button>
      </ScreenHeader>
      <div className="flex-1 flex min-h-0">
        <div className="w-56 shrink-0 border-r border-line p-3 space-y-4 overflow-auto">
          <div>
            <div className="text-[11px] font-semibold uppercase tracking-wider text-faint px-2 mb-1">Templates</div>
            {templates.map((t) => (
              <button key={t.id} onClick={() => load(t)} className={cn("w-full text-left h-8 px-2.5 rounded-lg text-[13px]", current.id === t.id ? "bg-panel-2 font-medium" : "text-muted hover:bg-panel-2")}>{t.display_name}</button>
            ))}
          </div>
          <div>
            <div className="text-[11px] font-semibold uppercase tracking-wider text-faint px-2 mb-1">{wsId ? "This workspace" : "Saved"}</div>
            {saved.length === 0 && <div className="px-2 text-[12px] text-faint">Copy a template to start.</div>}
            {saved.map((w) => (
              <div key={w.id} className={cn("group flex items-center gap-1 h-8 px-2.5 rounded-lg", current.id === w.id ? "bg-panel-2" : "hover:bg-panel-2")} onClick={() => load(w)}>
                <InlineEdit value={w.display_name} className="flex-1 text-[13px]" onSave={(v) => persist({ ...w, display_name: v })} />
                <button className="opacity-0 group-hover:opacity-100 text-faint hover:text-bad" onClick={async (e) => { e.stopPropagation(); await api.workflowDelete(wsId, w.id); reload(); }}><Trash2 className="h-3.5 w-3.5" /></button>
              </div>
            ))}
          </div>
          <Card className="p-3 text-[11.5px] text-muted space-y-1">
            <div className="font-semibold text-fg flex items-center gap-1"><Wand2 className="h-3.5 w-3.5" /> Compiles to</div>
            <div>High → {compiled.high}, Low → {compiled.low}, Trivial → {compiled.trivial}</div>
            <div>Review: {compiled.review ? "yes" : "no"} · Escalation: {compiled.escalation}</div>
          </Card>
        </div>
        <div className="flex-1 min-w-0 relative">
          {editable && (
            <div className="absolute z-10 top-3 left-3 flex gap-1 bg-elev border border-line rounded-xl p-1 shadow-sm">
              {(Object.keys(KIND_META) as NodeKind[]).map((k) => {
                const M = KIND_META[k];
                return <Button key={k} size="sm" variant="ghost" onClick={() => addNode(k)} title={`Add ${M.label}`}><Plus className="h-3 w-3" /><M.icon className="h-3.5 w-3.5" style={{ color: M.color }} />{M.label}</Button>;
              })}
            </div>
          )}
          <ReactFlow
            nodes={nodes}
            edges={edges.map((e) => ({ ...e, animated: e.label === "fail", style: { stroke: e.label === "fail" ? "var(--bad)" : e.label === "pass" ? "var(--ok)" : "var(--line-strong)" } }))}
            onNodesChange={(c) => { onNodesChange(c); if (c.some((x) => x.type === "position" || x.type === "remove")) setDirty(true); }}
            onEdgesChange={(c) => { onEdgesChange(c); setDirty(true); }}
            onConnect={onConnect}
            onNodeClick={(_, n) => setSel(n.id)}
            onEdgeClick={(_, e) => setSel(e.id)}
            onPaneClick={() => setSel(null)}
            nodeTypes={nodeTypes}
            nodesDraggable={editable}
            nodesConnectable={editable}
            elementsSelectable
            fitView
            proOptions={{ hideAttribution: true }}
            colorMode={document.documentElement.classList.contains("dark") ? "dark" : "light"}
          >
            <Background gap={18} color="var(--line)" />
            <Controls showInteractive={false} />
            <MiniMap pannable zoomable className="!bg-panel" />
          </ReactFlow>
        </div>
        {(selNode || selEdge) && (
          <div className="w-64 shrink-0 border-l border-line p-4 space-y-3 overflow-auto">
            {selNode && <NodeInspector node={selNode} editable={editable} update={update} onDelete={() => { setNodes((ns) => ns.filter((n) => n.id !== sel)); setSel(null); setDirty(true); }} />}
            {selEdge && (
              <div className="space-y-2">
                <div className="font-semibold">Edge</div>
                <Select className="w-full" disabled={!editable} value={(selEdge.label as string) ?? ""} onChange={(e) => { const label = e.target.value || undefined; setEdges((es) => es.map((x) => (x.id === sel ? { ...x, label, data: { branch: label } } : x))); setDirty(true); }}>
                  <option value="">Always</option>
                  <option value="pass">On pass</option>
                  <option value="fail">On fail</option>
                  <option value="high">Class: high</option>
                  <option value="low">Class: low</option>
                  <option value="trivial">Class: trivial</option>
                </Select>
                {editable && <Button size="sm" variant="ghost" className="hover:text-bad" onClick={() => { setEdges((es) => es.filter((x) => x.id !== sel)); setSel(null); setDirty(true); }}><Trash2 className="h-3.5 w-3.5" /> Delete edge</Button>}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function NodeInspector({ node, editable, update, onDelete }: { node: Node<NodeData>; editable: boolean; update: (p: Partial<NodeData>) => void; onDelete: () => void }) {
  const agents = useApp((s) => s.agents);
  const d = node.data;
  const runs = d.kind === "execute" || d.kind === "plan" || d.kind === "review";
  return (
    <div className="space-y-3 text-[12.5px]">
      <div className="flex items-center gap-2"><Badge>{KIND_META[d.kind].label}</Badge>{!editable && <span className="text-faint text-xs">template (read-only)</span>}</div>
      <label className="block">
        <div className="text-xs text-muted mb-1">Label</div>
        <input disabled={!editable} value={d.label} onChange={(e) => update({ label: e.target.value })} className="h-8 w-full rounded-lg bg-panel border border-line px-2 outline-none focus:border-accent disabled:opacity-60" />
      </label>
      {runs && (
        <>
          <label className="block">
            <div className="text-xs text-muted mb-1">Agent</div>
            <Select className="w-full" disabled={!editable} value={d.agent} onChange={(e) => update({ agent: e.target.value })}>
              <option value="router">Router decides</option>
              {agents.filter((a) => a.installed).map((a) => <option key={a.id} value={a.id}>{a.display_name}</option>)}
            </Select>
          </label>
          <label className="block">
            <div className="text-xs text-muted mb-1">Model tier</div>
            <Select className="w-full" disabled={!editable} value={d.tier} onChange={(e) => update({ tier: e.target.value as NodeData["tier"] })}>
              <option value="any">Any</option>
              <option value="local">Local</option>
              <option value="cheap_cloud">Cheap (local or cloud)</option>
              <option value="premium">Paid</option>
            </Select>
          </label>
        </>
      )}
      {d.kind === "execute" && (
        <div>
          <div className="text-xs text-muted mb-1">Handles step classes</div>
          {(["high", "low", "trivial"] as StepClass[]).map((c) => (
            <label key={c} className="flex items-center gap-2 h-7">
              <input type="checkbox" disabled={!editable} checked={!!d.classes?.includes(c)} onChange={(e) => update({ classes: e.target.checked ? [...(d.classes ?? []), c] : (d.classes ?? []).filter((x) => x !== c) })} />
              {c}
            </label>
          ))}
        </div>
      )}
      {d.kind === "plan" && (
        <label className="flex items-center gap-2"><input type="checkbox" disabled={!editable} checked={!!d.shortPlan} onChange={(e) => update({ shortPlan: e.target.checked })} /> Short plan</label>
      )}
      {editable && <Button size="sm" variant="ghost" className="hover:text-bad" onClick={onDelete}><Trash2 className="h-3.5 w-3.5" /> Delete node</Button>}
    </div>
  );
}
