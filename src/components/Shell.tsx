import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { Activity, BookOpen, Boxes, FolderDown, GitBranch, Home, Settings as Cog, Workflow, X, Zap } from "lucide-react";
import { useApp, type Screen } from "../lib/store";
import { api } from "../lib/api";
import { Button, Dialog } from "./ui";
import { cn } from "../lib/format";

const NAV: { id: Screen; label: string; icon: typeof Home; key: string }[] = [
  { id: "home", label: "Home", icon: Home, key: "1" },
  { id: "library", label: "Library", icon: BookOpen, key: "2" },
  { id: "agents", label: "Agents & Models", icon: Boxes, key: "3" },
  { id: "telemetry", label: "Telemetry", icon: Activity, key: "4" },
  { id: "workflows", label: "Workflows", icon: Workflow, key: "5" },
  { id: "settings", label: "Settings", icon: Cog, key: "6" },
];

export function Sidebar() {
  const screen = useApp((s) => s.screen);
  const go = useApp((s) => s.go);
  const brand = useApp((s) => s.brand);
  const runs = useApp((s) => s.runs);
  const viewRun = useApp((s) => s.viewRun);
  const viewRunId = useApp((s) => s.viewRunId);
  const workspace = useApp((s) => s.workspace);
  const active = Object.values(runs)
    .filter((r) => ["pending", "planning", "running", "reviewing"].includes(r.status))
    .sort((a, b) => b.started_at - a.started_at);

  return (
    <aside className="w-[208px] shrink-0 bg-panel border-r border-line flex flex-col">
      <div data-tauri-drag-region className="h-[52px] shrink-0" />
      <div className="px-3 pb-3">
        <div className="flex items-center gap-2 px-1.5">
          <Logo className="h-6 w-6" />
          <div className="min-w-0">
            <div className="text-[13px] font-semibold truncate">{brand.shortName}</div>
            {workspace && (
              <div className="text-[11px] text-faint truncate flex items-center gap-1">
                {workspace.info.is_git && <GitBranch className="h-3 w-3" />}
                {workspace.row.display_name}
              </div>
            )}
          </div>
        </div>
      </div>
      <nav className="px-2 flex flex-col gap-0.5">
        {NAV.map((n) => (
          <button
            key={n.id}
            onClick={() => go(n.id)}
            title={`⌘${n.key}`}
            className={cn(
              "flex items-center gap-2.5 h-8 px-2.5 rounded-lg text-[13px] font-medium transition-colors",
              screen === n.id ? "bg-accent-soft text-accent" : "text-muted hover:text-fg hover:bg-panel-2",
            )}
          >
            <n.icon className="h-4 w-4" />
            {n.label}
          </button>
        ))}
      </nav>
      {active.length > 0 && (
        <div className="mt-5 px-2">
          <div className="px-2.5 mb-1 text-[11px] font-semibold uppercase tracking-wider text-faint">Active runs</div>
          {active.map((r) => (
            <button
              key={r.id}
              onClick={() => viewRun(r.id)}
              className={cn(
                "w-full flex items-center gap-2 h-8 px-2.5 rounded-lg text-left text-[12.5px]",
                screen === "run" && viewRunId === r.id ? "bg-panel-2 text-fg" : "text-muted hover:bg-panel-2 hover:text-fg",
              )}
            >
              <span className="h-1.5 w-1.5 rounded-full bg-info animate-pulse-soft shrink-0" />
              <span className="truncate">{r.goal}</span>
            </button>
          ))}
        </div>
      )}
      <div className="mt-auto p-3 text-[11px] text-faint">{brand.productName}</div>
    </aside>
  );
}

export function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 1024 1024" className={className} aria-hidden>
      <rect x="64" y="64" width="896" height="896" rx="200" fill="#1e1b4b" />
      <path d="M332 512 C 470 512, 520 340, 690 340" stroke="#a78bfa" strokeWidth="44" fill="none" strokeLinecap="round" />
      <path d="M332 512 C 470 512, 520 684, 690 684" stroke="#5eead4" strokeWidth="44" fill="none" strokeLinecap="round" />
      <circle cx="300" cy="512" r="96" fill="#f8fafc" />
      <circle cx="724" cy="340" r="84" fill="#8b5cf6" />
      <circle cx="724" cy="684" r="84" fill="#14b8a6" />
    </svg>
  );
}

/** Toasts (bottom-right) and the memory-pressure banner. */
export function Notices() {
  const notices = useApp((s) => s.notices);
  const dismiss = useApp((s) => s.dismiss);
  const toast = useApp((s) => s.toast);
  const banner = [...notices].reverse().find((n) => n.level.startsWith("pressure"));
  const toasts = notices.filter((n) => !n.level.startsWith("pressure")).slice(-3);
  return (
    <>
      {banner && (
        <div className={cn("flex items-center gap-3 px-4 py-2 text-[12.5px] border-b", banner.level === "pressure_critical" ? "bg-bad-soft text-bad border-bad/20" : "bg-warn-soft text-warn border-warn/20")}>
          <Zap className="h-4 w-4 shrink-0" />
          <span className="flex-1">{banner.text}</span>
          <Button
            size="sm"
            variant="outline"
            onClick={async () => {
              const snap = await api.resourceSnapshot();
              let freed = 0;
              for (const m of snap.models) freed += await api.unloadModel(m.name).catch(() => 0);
              toast("info", freed > 0 ? `Unloaded local models (${freed.toFixed(1)} GB freed).` : "No local models were loaded.");
              dismiss(banner.id);
            }}
          >
            Unload models
          </Button>
          <button onClick={() => dismiss(banner.id)} className="opacity-70 hover:opacity-100" aria-label="Dismiss">
            <X className="h-4 w-4" />
          </button>
        </div>
      )}
      <div className="fixed bottom-10 right-4 z-40 flex flex-col gap-2 w-[340px]">
        {toasts.map((n) => (
          <div
            key={n.id}
            className={cn(
              "animate-fade-in rounded-xl border shadow-lg px-3.5 py-2.5 text-[12.5px] bg-elev flex gap-2 items-start",
              n.level === "error" ? "border-bad/40" : n.level === "warn" ? "border-warn/40" : "border-line",
            )}
          >
            <span className={cn("mt-1 h-2 w-2 rounded-full shrink-0", n.level === "error" ? "bg-bad" : n.level === "warn" ? "bg-warn" : "bg-info")} />
            <span className="flex-1 leading-relaxed selectable">{n.text}</span>
            {n.action && (
              <Button size="sm" variant="ghost" onClick={n.action.run}>
                {n.action.label}
              </Button>
            )}
            <button onClick={() => dismiss(n.id)} className="text-faint hover:text-fg" aria-label="Dismiss">
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        ))}
      </div>
    </>
  );
}

/** Questions the core is waiting on (approvals, escalation, failures). */
export function Approvals() {
  const approvals = useApp((s) => s.approvals);
  const viewRun = useApp((s) => s.viewRun);
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const a = approvals[0];
  if (!a) return null;
  const answer = async (opt: string) => {
    setBusy(true);
    try {
      await api.answerApproval(a.id, opt);
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      dismissable={false}
      onClose={() => {}}
      title={a.title}
      width={520}
      footer={
        <>
          <Button variant="ghost" onClick={() => viewRun(a.run_id)} className="mr-auto">
            View run
          </Button>
          {a.options.map((o) => (
            <Button key={o.id} variant={o.primary ? "primary" : o.danger ? "outline" : "secondary"} className={o.danger ? "text-bad" : undefined} disabled={busy} onClick={() => answer(o.id)}>
              {o.label}
            </Button>
          ))}
        </>
      }
    >
      <p className="text-muted leading-relaxed selectable">{a.body}</p>
      {approvals.length > 1 && <p className="text-xs text-faint mt-3">{approvals.length - 1} more waiting</p>}
    </Dialog>
  );
}

/** Drag a folder onto the window to open it as the workspace. */
export function DropZone() {
  const [over, setOver] = useState(false);
  const openFolder = useApp((s) => s.openFolder);
  const go = useApp((s) => s.go);
  useEffect(() => {
    let un: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setOver(true);
        else if (e.payload.type === "leave") setOver(false);
        else if (e.payload.type === "drop") {
          setOver(false);
          const p = e.payload.paths[0];
          if (p) void openFolder(p).then((w) => w && go("home"));
        }
      })
      .then((f) => (un = f), () => {});
    return () => un?.();
  }, [openFolder, go]);
  if (!over) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-accent/10 backdrop-blur-[1px] pointer-events-none">
      <div className="rounded-2xl border-2 border-dashed border-accent bg-elev px-10 py-8 text-center shadow-2xl">
        <FolderDown className="h-8 w-8 mx-auto text-accent" />
        <div className="mt-2 font-semibold">Drop a folder to open it</div>
        <div className="text-muted text-xs mt-1">Agents will work inside it.</div>
      </div>
    </div>
  );
}

export async function pickFolder(): Promise<string | null> {
  const r = await open({ directory: true, multiple: false, title: "Open a folder" });
  return typeof r === "string" ? r : null;
}

/** Global shortcuts: ⌘1–6 switch screens, ⌘O opens a folder. */
export function useShortcuts() {
  const go = useApp((s) => s.go);
  const openFolder = useApp((s) => s.openFolder);
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (!e.metaKey && !e.ctrlKey) return;
      const n = NAV.find((x) => x.key === e.key);
      if (n) {
        e.preventDefault();
        go(n.id);
      } else if (e.key === "o") {
        e.preventDefault();
        void pickFolder().then((p) => {
          if (p) void openFolder(p).then(() => go("home"));
        });
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [go, openFolder]);
}

/** Top bar for each screen: title + actions; doubles as the window drag region. */
export function ScreenHeader({ title, subtitle, children }: { title: React.ReactNode; subtitle?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div data-tauri-drag-region className="h-[52px] shrink-0 flex items-center gap-3 px-6 border-b border-line bg-bg/80 backdrop-blur">
      <div data-tauri-drag-region className="min-w-0 flex-1">
        <div data-tauri-drag-region className="text-[15px] font-semibold truncate">{title}</div>
        {subtitle && <div data-tauri-drag-region className="text-[11.5px] text-muted truncate">{subtitle}</div>}
      </div>
      {children}
    </div>
  );
}
