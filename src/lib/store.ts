import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type * as T from "./types";
import { logStore } from "./logs";

export type Screen = "home" | "run" | "library" | "agents" | "telemetry" | "workflows" | "settings";

export interface Notice {
  id: number;
  level: string;
  text: string;
  run_id: string | null;
  ts: number;
  action?: { label: string; run: () => void };
}

interface InstallState {
  lines: string[];
  done: boolean | null;
}

interface PullState {
  status: string;
  completed: number;
  total: number;
  done: boolean | null;
  error: string | null;
}

interface State {
  ready: boolean;
  bootError: string | null;
  brand: T.Brand;
  catalog: T.Catalog;
  os: string;
  version: string;
  dataDir: string;
  settings: T.Settings | null;
  agents: T.AgentInfo[];
  screen: Screen;
  viewRunId: string | null;
  workspace: T.OpenedWorkspace | null;
  recent: T.WorkspaceRow[];
  runs: Record<string, T.RunRow>;
  steps: Record<string, Record<string, T.StepRow>>;
  approvals: T.ApprovalRequest[];
  notices: Notice[];
  snapshot: T.Snapshot | null;
  installs: Record<string, InstallState>;
  pulls: Record<string, PullState>;
  move: { completed: number; total: number; done: boolean | null; error: string | null } | null;
  libraryVersion: number;
  agentsVersion: number;
  celebrate: boolean;
  onboardingOpen: boolean;

  go: (s: Screen) => void;
  viewRun: (runId: string) => void;
  setSettings: (s: T.Settings) => void;
  saveSettings: (patch: Partial<T.Settings>) => Promise<void>;
  setWorkspace: (w: T.OpenedWorkspace | null) => void;
  openFolder: (path: string) => Promise<T.OpenedWorkspace | null>;
  refreshRecent: () => Promise<void>;
  refreshAgents: (refresh?: boolean, auth?: boolean) => Promise<void>;
  loadRun: (runId: string) => Promise<void>;
  toast: (level: string, text: string, action?: Notice["action"]) => void;
  dismiss: (id: number) => void;
  applyBatch: (batch: T.UiEvent[]) => void;
}

let noticeId = 1;

const emptyBrand: T.Brand = { productName: "", shortName: "", handoffDirName: ".orchestrator", cliName: "orch" };

export const useApp = create<State>((set, get) => ({
  ready: false,
  bootError: null,
  brand: emptyBrand,
  catalog: { agents: [], runtimes: [], models: [], providers: [] },
  os: "macos",
  version: "",
  dataDir: "",
  settings: null,
  agents: [],
  screen: "home",
  viewRunId: null,
  workspace: null,
  recent: [],
  runs: {},
  steps: {},
  approvals: [],
  notices: [],
  snapshot: null,
  installs: {},
  pulls: {},
  move: null,
  libraryVersion: 0,
  agentsVersion: 0,
  celebrate: false,
  onboardingOpen: false,

  go: (screen) => set({ screen }),
  viewRun: (runId) => {
    set({ screen: "run", viewRunId: runId });
    if (!get().runs[runId]) void get().loadRun(runId);
  },
  setSettings: (settings) => set({ settings }),
  saveSettings: async (patch) => {
    const cur = get().settings;
    if (!cur) return;
    const next = { ...cur, ...patch };
    set({ settings: next });
    try {
      set({ settings: await api.saveSettings(next) });
    } catch (e) {
      set({ settings: cur });
      get().toast("error", `Couldn't save settings: ${String(e)}`);
    }
  },
  setWorkspace: (workspace) => {
    set({ workspace });
    try {
      if (workspace) localStorage.setItem("lastWorkspace", workspace.row.path);
    } catch {
      /* storage unavailable */
    }
  },
  openFolder: async (path) => {
    try {
      const w = await api.openWorkspace(path);
      get().setWorkspace(w);
      void get().refreshRecent();
      return w;
    } catch (e) {
      get().toast("error", String(e));
      return null;
    }
  },
  refreshRecent: async () => set({ recent: await api.recentWorkspaces() }),
  refreshAgents: async (refresh = false, auth = false) => {
    const agents = await api.agents(refresh, auth);
    set((s) => ({ agents, agentsVersion: s.agentsVersion + 1 }));
  },
  loadRun: async (runId) => {
    const d = await api.getRun(runId);
    set((s) => ({
      runs: { ...s.runs, [runId]: d.run },
      steps: { ...s.steps, [runId]: Object.fromEntries(d.steps.map((x) => [x.id, x])) },
    }));
  },
  toast: (level, text, action) => {
    const n: Notice = { id: noticeId++, level, text, run_id: null, ts: Date.now(), action };
    set((s) => ({ notices: [...s.notices.slice(-4), n] }));
    if (level !== "error" && !level.startsWith("pressure")) {
      setTimeout(() => get().dismiss(n.id), 6000);
    }
  },
  dismiss: (id) => set((s) => ({ notices: s.notices.filter((n) => n.id !== id) })),

  applyBatch: (batch) => {
    // One state update per batch (the core sends at most one batch per 50 ms).
    const s = get();
    let runs = s.runs;
    let steps = s.steps;
    let approvals = s.approvals;
    let notices = s.notices;
    let snapshot = s.snapshot;
    let installs = s.installs;
    let pulls = s.pulls;
    let move = s.move;
    let libraryVersion = s.libraryVersion;
    let agentsDirty = false;
    let celebrate = s.celebrate;
    const logLines: [string, T.LogLine][] = [];
    for (const e of batch) {
      switch (e.type) {
        case "run": {
          const prev = runs[e.run.id];
          runs = { ...runs, [e.run.id]: e.run };
          if (prev && prev.status !== "succeeded" && e.run.status === "succeeded") {
            try {
              if (!localStorage.getItem("celebrated")) {
                celebrate = true;
                localStorage.setItem("celebrated", "1");
              }
            } catch {
              /* ignore */
            }
          }
          break;
        }
        case "step": {
          const r = { ...(steps[e.step.run_id] ?? {}), [e.step.id]: e.step };
          steps = { ...steps, [e.step.run_id]: r };
          break;
        }
        case "log":
          logLines.push([e.step_id, e.line]);
          break;
        case "notice": {
          const n: Notice = { id: noticeId++, level: e.level, text: e.text, run_id: e.run_id, ts: Date.now() };
          notices = [...notices.slice(-4), n];
          if (e.level === "info" || e.level === "warn") {
            setTimeout(() => get().dismiss(n.id), e.level === "info" ? 6000 : 10000);
          }
          break;
        }
        case "approval":
          approvals = [...approvals.filter((a) => a.id !== e.request.id), e.request];
          break;
        case "approval_done":
          approvals = approvals.filter((a) => a.id !== e.id);
          break;
        case "resources":
          snapshot = e.snapshot;
          break;
        case "install": {
          const cur = installs[e.id] ?? { lines: [], done: null };
          const lines = cur.lines.length > 400 ? [...cur.lines.slice(-300), e.line] : [...cur.lines, e.line];
          installs = { ...installs, [e.id]: { lines, done: e.done ?? cur.done } };
          break;
        }
        case "pull":
          pulls = { ...pulls, [e.model]: { status: e.status, completed: e.completed, total: e.total, done: e.done, error: e.error } };
          if (e.done) agentsDirty = true;
          break;
        case "move":
          move = { completed: e.completed, total: e.total, done: e.done, error: e.error };
          break;
        case "library":
          libraryVersion++;
          break;
        case "agents":
          agentsDirty = true;
          break;
      }
    }
    if (logLines.length) logStore.append(logLines);
    set({ runs, steps, approvals, notices, snapshot, installs, pulls, move, libraryVersion, celebrate });
    if (agentsDirty) void get().refreshAgents();
  },
}));

export async function boot() {
  const b = await api.bootstrap();
  useApp.setState({
    brand: b.brand,
    catalog: b.catalog,
    os: b.os,
    version: b.version,
    dataDir: b.data_dir,
    settings: b.settings,
    agents: b.agents,
    recent: b.recent,
    approvals: b.approvals,
    onboardingOpen: !b.settings.onboarded,
  });
  await listen<T.UiEvent[]>("orch://events", (ev) => useApp.getState().applyBatch(ev.payload));
  await listen<string>("orch://open-path", (ev) => {
    void useApp.getState().openFolder(ev.payload).then(() => useApp.getState().go("home"));
  });
  for (const id of b.active_runs) void useApp.getState().loadRun(id);
  // Reopen the last workspace (or a folder passed on the command line).
  const pending = await api.takePendingOpen();
  let last: string | null = null;
  try {
    last = localStorage.getItem("lastWorkspace");
  } catch {
    /* ignore */
  }
  const path = pending ?? last ?? b.recent[0]?.path ?? null;
  if (path) {
    try {
      useApp.getState().setWorkspace(await api.openWorkspace(path));
    } catch {
      /* folder gone */
    }
  }
  useApp.setState({ ready: true });
  // Status bar starts with a one-shot snapshot (no timers while idle).
  api.resourceSnapshot().then((snapshot) => useApp.setState({ snapshot }), () => {});
}

export const tierLabel: Record<T.Tier, string> = { local: "Local", cheap_cloud: "Cheap cloud", premium: "Premium" };

export function modeName(settings: T.Settings | null, id: string): string {
  return settings?.modes.find((m) => m.id === id)?.display_name ?? id;
}

export function agentName(agents: T.AgentInfo[], id: string | null | undefined): string {
  if (!id) return "—";
  return agents.find((a) => a.id === id)?.display_name ?? id;
}
