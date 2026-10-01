import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { BookOpen, Bot, Copy, FileWarning, FolderInput, Globe2, Import, Layers, Plus, RefreshCw, Save, Scroll, Trash2, Wand2 } from "lucide-react";
import { useApp, agentName } from "../lib/store";
import { api, errorText } from "../lib/api";
import type { LibraryItem, LibraryKind, LibraryScope, LibraryView, Preview } from "../lib/types";
import { Badge, Button, Card, Dialog, Empty, InlineEdit, Select, Spinner, Switch, Tabs } from "../components/ui";
import { ScreenHeader } from "../components/Shell";
import { cn } from "../lib/format";

const Editor = lazy(() => import("./Editor"));

const KINDS: { kind: LibraryKind; label: string; icon: typeof Scroll; hint: string }[] = [
  { kind: "rule", label: "Rules", icon: Scroll, hint: "Always-on or file-scoped instructions" },
  { kind: "skill", label: "Skills", icon: Wand2, hint: "Reusable capabilities loaded on demand" },
  { kind: "agent", label: "Agents", icon: Bot, hint: "Named roles with a prompt, tier and attached skills/rules" },
];

export default function Library() {
  const workspace = useApp((s) => s.workspace);
  const setWorkspace = useApp((s) => s.setWorkspace);
  const version = useApp((s) => s.libraryVersion);
  const toast = useApp((s) => s.toast);
  const wsId = workspace?.row.id ?? null;
  const [view, setView] = useState<LibraryView | null>(null);
  const [selPath, setSelPath] = useState<string | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const [creating, setCreating] = useState<LibraryKind | null>(null);
  const [profiles, setProfiles] = useState(false);
  const [syncing, setSyncing] = useState(false);

  const reload = useCallback(() => {
    api.libraryList(wsId).then(setView, (e) => toast("error", errorText(e)));
  }, [wsId, toast]);
  useEffect(reload, [reload, version]);

  const items = useMemo(() => view?.items ?? [], [view]);
  const sel = items.find((i) => i.path === selPath) ?? null;
  useEffect(() => {
    if (!sel && items.length) setSelPath(items[0].path);
  }, [items, sel]);
  useEffect(() => setDraft(null), [selPath]);

  const dirty = draft !== null && sel && draft !== sel.raw;
  const saveItem = async () => {
    if (!sel || draft === null) return;
    try {
      const report = await api.librarySave(sel.path, draft, wsId);
      setDraft(null);
      toast("info", report ? `Saved and synced (${report.written.length} file${report.written.length === 1 ? "" : "s"} updated).` : "Saved.");
      reload();
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const sync = async () => {
    setSyncing(true);
    try {
      const r = await api.librarySync(wsId);
      toast(r.conflicts.length ? "warn" : "info", `Synced: ${r.written.length} written, ${r.removed.length} removed, ${r.unchanged} unchanged${r.conflicts.length ? `, ${r.conflicts.length} left alone because you edited them` : ""}.`);
      reload();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setSyncing(false);
    }
  };

  const profileId = workspace?.row.library_profile_id ?? "";

  return (
    <div className="flex flex-col h-full">
      <ScreenHeader title="Library" subtitle="Write a skill, agent or rule once; every installed agent CLI gets it in its own format.">
        {workspace && (
          <Select
            className="no-drag"
            value={profileId}
            title="Library profile for this workspace"
            onChange={async (e) => {
              const row = await api.updateWorkspace({ ...workspace.row, library_profile_id: e.target.value || null });
              setWorkspace({ ...workspace, row });
            }}
          >
            <option value="">Profile: everything</option>
            {view?.profiles.map((p) => <option key={p.id} value={p.id}>Profile: {p.display_name}</option>)}
          </Select>
        )}
        <Button className="no-drag" variant="ghost" onClick={() => setProfiles(true)}><Layers className="h-3.5 w-3.5" /> Profiles</Button>
        {workspace && view && view.import_candidates.length > 0 && (
          <Button
            className="no-drag"
            onClick={async () => {
              const n = await api.libraryImport(workspace.row.id, "workspace");
              toast("info", `Imported ${n} item${n === 1 ? "" : "s"}.`);
            }}
          >
            <Import className="h-3.5 w-3.5" /> Import ({view.import_candidates.length})
          </Button>
        )}
        <Button className="no-drag" onClick={sync} loading={syncing} disabled={!workspace}><RefreshCw className="h-3.5 w-3.5" /> Sync to workspace</Button>
      </ScreenHeader>

      {view && view.conflicts.length > 0 && workspace && (
        <div className="px-6 py-2 bg-warn-soft border-b border-warn/20 text-[12.5px] flex items-center gap-3 flex-wrap">
          <FileWarning className="h-4 w-4 text-warn" />
          <span>You edited {view.conflicts.length} generated file{view.conflicts.length === 1 ? "" : "s"}. They won't be overwritten.</span>
          {view.conflicts.slice(0, 3).map((c) => (
            <Button
              key={c.path}
              size="sm"
              variant="outline"
              onClick={async () => {
                const name = await api.libraryImportEdit(workspace.row.id, c.path);
                toast("info", name ? `Imported your edit into "${name}".` : "No Library item matches that file.");
              }}
            >
              Import {c.path.split("/").pop()}
            </Button>
          ))}
        </div>
      )}

      <div className="flex-1 flex min-h-0">
        <div className="w-[280px] shrink-0 border-r border-line overflow-auto p-3 space-y-4">
          {!view && <div className="p-4 text-faint"><Spinner /></div>}
          {KINDS.map(({ kind, label, icon: Icon, hint }) => {
            const list = items.filter((i) => i.kind === kind);
            return (
              <div key={kind}>
                <div className="flex items-center justify-between px-2 mb-1">
                  <span className="text-[11px] font-semibold uppercase tracking-wider text-faint flex items-center gap-1.5" title={hint}>
                    <Icon className="h-3.5 w-3.5" /> {label} <span className="font-normal">{list.length}</span>
                  </span>
                  <button className="text-faint hover:text-fg p-0.5 rounded" title={`New ${kind}`} onClick={() => setCreating(kind)}>
                    <Plus className="h-3.5 w-3.5" />
                  </button>
                </div>
                {list.length === 0 && <div className="px-2 py-1 text-[12px] text-faint">None yet</div>}
                {list.map((i) => (
                  <ItemRow key={i.path} item={i} selected={i.path === selPath} onSelect={() => setSelPath(i.path)} wsId={wsId} onChange={reload} />
                ))}
              </div>
            );
          })}
          {!workspace && <Card className="p-3 text-[12px] text-muted">Open a workspace to add workspace-scoped items and sync them into the project.</Card>}
        </div>

        <div className="flex-1 min-w-0 flex flex-col">
          {sel ? (
            <>
              <div className="h-11 shrink-0 flex items-center gap-2 px-4 border-b border-line">
                <span className="font-semibold truncate">{sel.display_name}</span>
                <span className="text-xs text-faint font-mono">{sel.id}</span>
                <ScopeBadge scope={sel.scope} />
                {sel.overridden && <Badge tone="warn" title="A workspace item with the same id overrides this one">overridden</Badge>}
                <span className="ml-auto flex items-center gap-1">
                  {dirty && <span className="text-xs text-warn mr-1">Unsaved</span>}
                  <Button size="sm" variant={dirty ? "primary" : "secondary"} disabled={!dirty} onClick={saveItem} title="⌘S"><Save className="h-3.5 w-3.5" /> Save</Button>
                  <ItemActions item={sel} wsId={wsId} onDone={(p) => { if (p) setSelPath(p); reload(); }} />
                </span>
              </div>
              <div className="flex-1 min-h-0">
                <Suspense fallback={<div className="p-4 text-faint"><Spinner /></div>}>
                  <Editor key={sel.path} value={draft ?? sel.raw} onChange={setDraft} onSave={saveItem} />
                </Suspense>
              </div>
            </>
          ) : (
            <Empty icon={<BookOpen className="h-8 w-8" />} title="Your Library is empty" action={<Button variant="primary" onClick={() => setCreating("rule")}><Plus className="h-4 w-4" /> New rule</Button>}>
              Rules, skills and agents you write here are synced into Claude Code, Cursor, Codex, Copilot, OpenCode, Aider and Goose.
            </Empty>
          )}
        </div>

        {sel && view && <PreviewPane item={sel} view={view} wsId={wsId} draft={draft} />}
      </div>

      {creating && <CreateDialog kind={creating} wsId={wsId} onClose={() => setCreating(null)} onCreated={(i) => { setSelPath(i.path); reload(); }} />}
      {profiles && view && <ProfilesDialog view={view} onClose={() => { setProfiles(false); reload(); }} />}
    </div>
  );
}

function ScopeBadge({ scope }: { scope: LibraryScope }) {
  return scope === "global" ? <Badge><Globe2 className="h-3 w-3" />Global</Badge> : <Badge tone="accent"><FolderInput className="h-3 w-3" />Workspace</Badge>;
}

function ItemRow({ item, selected, onSelect, wsId, onChange }: { item: LibraryItem; selected: boolean; onSelect: () => void; wsId: string | null; onChange: () => void }) {
  const toast = useApp((s) => s.toast);
  return (
    <div
      onClick={onSelect}
      className={cn("group flex items-center gap-2 h-8 px-2 rounded-lg cursor-default", selected ? "bg-panel-2" : "hover:bg-panel-2/60", (item.overridden || !item.enabled) && "opacity-55")}
    >
      <Switch
        checked={item.enabled}
        label="Enabled"
        onChange={async (v) => {
          try {
            await api.libraryToggle(item.path, v, wsId);
            onChange();
          } catch (e) {
            toast("error", errorText(e));
          }
        }}
      />
      <InlineEdit
        value={item.display_name}
        className="flex-1 min-w-0 text-[13px]"
        onSave={async (v) => {
          await api.libraryRename(item.path, v, wsId);
          onChange();
        }}
      />
      <span className={cn("h-1.5 w-1.5 rounded-full shrink-0", item.scope === "global" ? "bg-faint" : "bg-accent")} title={item.scope === "global" ? "Global" : "Workspace"} />
    </div>
  );
}

function ItemActions({ item, wsId, onDone }: { item: LibraryItem; wsId: string | null; onDone: (newPath?: string) => void }) {
  const toast = useApp((s) => s.toast);
  const [confirm, setConfirm] = useState<string[] | null>(null);
  const other: LibraryScope = item.scope === "global" ? "workspace" : "global";
  return (
    <>
      <Button size="icon" variant="ghost" title="Duplicate" onClick={async () => onDone(await api.libraryDuplicate(item.path, wsId))}><Copy className="h-3.5 w-3.5" /></Button>
      <Button
        size="sm"
        variant="ghost"
        disabled={other === "workspace" && !wsId}
        title={`Move to ${other}`}
        onClick={async () => {
          try {
            onDone(await api.libraryMove(item.path, other, false, wsId));
            toast("info", `Moved to ${other === "global" ? "Global" : "this workspace"}.`);
          } catch (e) {
            toast("error", errorText(e));
          }
        }}
      >
        {other === "global" ? <Globe2 className="h-3.5 w-3.5" /> : <FolderInput className="h-3.5 w-3.5" />}
        {other === "global" ? "Make global" : "Move to workspace"}
      </Button>
      <Button size="icon" variant="ghost" title="Delete" className="hover:text-bad" onClick={async () => setConfirm(await api.libraryReferences(item.path, wsId))}><Trash2 className="h-3.5 w-3.5" /></Button>
      <Dialog
        open={!!confirm}
        onClose={() => setConfirm(null)}
        title={`Delete "${item.display_name}"?`}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(null)}>Cancel</Button>
            <Button variant="danger" onClick={async () => { await api.libraryDelete(item.path, wsId); setConfirm(null); onDone(); }}>Delete</Button>
          </>
        }
      >
        {confirm && confirm.length > 0 ? (
          <p className="text-muted text-[12.5px]">These agents reference it and will lose it: <b className="text-fg">{confirm.join(", ")}</b>.</p>
        ) : (
          <p className="text-muted text-[12.5px]">The file is removed; generated copies are cleaned up on the next sync.</p>
        )}
      </Dialog>
    </>
  );
}

function PreviewPane({ item, view, wsId, draft }: { item: LibraryItem; view: LibraryView; wsId: string | null; draft: string | null }) {
  const agents = useApp((s) => s.agents);
  const [previews, setPreviews] = useState<Preview[]>([]);
  const [cli, setCli] = useState<string>("claude");
  useEffect(() => {
    api.libraryPreview(item.path, wsId).then(setPreviews, () => setPreviews([]));
  }, [item.path, item.raw, wsId]);
  const installed = useMemo(() => previews.filter((p) => p.installed).map((p) => p.cli), [previews]);
  useEffect(() => {
    if (installed.length && !installed.includes(cli)) setCli(installed[0]);
  }, [installed, cli]);
  const p = previews.find((x) => x.cli === cli);
  const status = (pv: Preview) => (!pv.installed ? "not installed" : pv.support === "fallback" ? "task-file fallback" : pv.support === "converted" ? "converted" : "synced");
  return (
    <div className="w-[340px] shrink-0 border-l border-line flex flex-col min-h-0">
      <div className="px-3 pt-2">
        <div className="text-[11px] font-semibold uppercase tracking-wider text-faint mb-1">What each CLI receives</div>
        <Tabs
          value={cli}
          onChange={setCli}
          tabs={previews.map((pv) => ({ value: pv.cli, label: <span className={cn(!pv.installed && "opacity-50")}>{agentName(agents, pv.cli).replace(" CLI", "").replace("GitHub ", "")}</span> }))}
        />
      </div>
      {p && (
        <div className="flex-1 min-h-0 flex flex-col">
          <div className="px-3 py-2 flex items-center gap-2 text-[11.5px]">
            <Badge tone={!p.installed ? "neutral" : p.support === "native" ? "ok" : p.support === "converted" ? "info" : "warn"}>{status(p)}</Badge>
            {draft !== null && draft !== item.raw && <span className="text-faint">preview updates after Save</span>}
          </div>
          <pre className="flex-1 overflow-auto mx-3 mb-3 p-3 rounded-lg bg-panel-2 text-[11.5px] font-mono whitespace-pre-wrap selectable">{p.text || "—"}</pre>
        </div>
      )}
      <div className="px-3 pb-3 text-[11px] text-faint">Sync targets: {view.targets.length ? view.targets.map((t) => agentName(agents, t)).join(", ") : "no agent CLIs installed"}</div>
    </div>
  );
}

function CreateDialog({ kind, wsId, onClose, onCreated }: { kind: LibraryKind; wsId: string | null; onClose: () => void; onCreated: (i: LibraryItem) => void }) {
  const toast = useApp((s) => s.toast);
  const [name, setName] = useState("");
  const [scope, setScope] = useState<LibraryScope>(wsId ? "workspace" : "global");
  const create = async () => {
    if (!name.trim()) return;
    try {
      onCreated(await api.libraryCreate(kind, scope, name.trim(), wsId));
      onClose();
    } catch (e) {
      toast("error", errorText(e));
    }
  };
  return (
    <Dialog open onClose={onClose} title={`New ${kind}`} footer={<><Button variant="ghost" onClick={onClose}>Cancel</Button><Button variant="primary" onClick={create} disabled={!name.trim()}>Create</Button></>}>
      <div className="space-y-3">
        <input autoFocus value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && create()} placeholder={kind === "rule" ? "e.g. Named exports only" : kind === "skill" ? "e.g. Write migration" : "e.g. Security reviewer"} className="h-9 w-full rounded-lg bg-panel border border-line px-3 outline-none focus:border-accent" />
        <div className="flex items-center gap-2 text-[12.5px]">
          <span className="text-muted">Scope</span>
          <Select value={scope} onChange={(e) => setScope(e.target.value as LibraryScope)}>
            <option value="global">Global (every workspace)</option>
            <option value="workspace" disabled={!wsId}>This workspace (can be committed)</option>
          </Select>
        </div>
      </div>
    </Dialog>
  );
}

function ProfilesDialog({ view, onClose }: { view: LibraryView; onClose: () => void }) {
  const toast = useApp((s) => s.toast);
  const [editing, setEditing] = useState<{ id: string | null; name: string; items: Set<string> } | null>(null);
  const key = (i: LibraryItem) => `${i.kind}s:${i.id}`;
  return (
    <Dialog open onClose={onClose} title="Library profiles" width={560} footer={<Button onClick={onClose}>Done</Button>}>
      {!editing ? (
        <div className="space-y-2">
          <p className="text-[12.5px] text-muted">Profiles are named sets of items (e.g. "Frontend", "Rust backend"). Each workspace picks one in the Library header.</p>
          {view.profiles.map((p) => (
            <div key={p.id} className="flex items-center gap-2 h-9 px-3 rounded-lg bg-panel-2">
              <span className="font-medium flex-1">{p.display_name}</span>
              <span className="text-xs text-faint">{p.item_ids.length} items</span>
              <Button size="sm" variant="ghost" onClick={() => setEditing({ id: p.id, name: p.display_name, items: new Set(p.item_ids) })}>Edit</Button>
              <Button size="icon" variant="ghost" onClick={async () => { await api.deleteProfile(p.id); onClose(); }}><Trash2 className="h-3.5 w-3.5" /></Button>
            </div>
          ))}
          <Button onClick={() => setEditing({ id: null, name: "", items: new Set(view.items.map(key)) })}><Plus className="h-3.5 w-3.5" /> New profile</Button>
        </div>
      ) : (
        <div className="space-y-3">
          <input autoFocus value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} placeholder="Profile name" className="h-9 w-full rounded-lg bg-panel border border-line px-3 outline-none focus:border-accent" />
          <div className="max-h-72 overflow-auto space-y-1">
            {view.items.filter((i) => !i.overridden).map((i) => (
              <label key={i.path} className="flex items-center gap-2 h-8 px-2 rounded hover:bg-panel-2">
                <input type="checkbox" checked={editing.items.has(key(i))} onChange={(e) => { const s = new Set(editing.items); if (e.target.checked) s.add(key(i)); else s.delete(key(i)); setEditing({ ...editing, items: s }); }} />
                <Badge>{i.kind}</Badge>
                <span>{i.display_name}</span>
              </label>
            ))}
          </div>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setEditing(null)}>Back</Button>
            <Button variant="primary" disabled={!editing.name.trim()} onClick={async () => { await api.saveProfile(editing.id, editing.name.trim(), [...editing.items]); toast("info", "Profile saved."); onClose(); }}>Save profile</Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
