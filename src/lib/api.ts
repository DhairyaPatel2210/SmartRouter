// Typed wrappers over the core's Tauri commands. Command arguments are
// camelCase here; Tauri maps them to the Rust snake_case parameters.
import { invoke } from "@tauri-apps/api/core";
import type * as T from "./types";

const call = <R>(cmd: string, args?: Record<string, unknown>) => invoke<R>(cmd, args);

export interface StartOpts {
  workspace_id: string;
  goal: string;
  mode?: string;
  dirty_strategy?: T.DirtyStrategy | null;
  init_git?: boolean;
  pool_override?: T.ExecutorEntry[] | null;
}

export interface Assignment {
  agent_id: string;
  model: T.ModelRef | null;
  library_agent?: string | null;
}

export const api = {
  bootstrap: () => call<T.Bootstrap>("bootstrap"),
  takePendingOpen: () => call<string | null>("take_pending_open"),
  getSettings: () => call<T.Settings>("get_settings"),
  saveSettings: (settings: T.Settings) => call<T.Settings>("save_settings", { settings }),
  scan: () => call<T.ScanResult>("scan"),
  detectRuntimes: () => call<T.DetectedRuntime[]>("detect_runtimes"),

  openWorkspace: (path: string) => call<T.OpenedWorkspace>("open_workspace", { path }),
  inspectWorkspace: (id: string) => call<T.OpenedWorkspace>("inspect_workspace", { id }),
  recentWorkspaces: () => call<T.WorkspaceRow[]>("recent_workspaces"),
  updateWorkspace: (row: T.WorkspaceRow) => call<T.WorkspaceRow>("update_workspace", { row }),
  removeWorkspace: (id: string) => call<void>("remove_workspace", { id }),
  initGit: (id: string) => call<T.OpenedWorkspace>("init_git", { id }),
  sampleProject: () => call<T.OpenedWorkspace>("sample_project"),

  preflight: (workspaceId: string | null, poolOverride?: T.ExecutorEntry[] | null) =>
    call<T.PreflightView>("preflight", { workspaceId, poolOverride: poolOverride ?? null }),
  startRun: (opts: StartOpts) => call<string>("start_run", { opts }),
  cancelRun: (runId: string) => call<void>("cancel_run", { runId }),
  pauseRun: (runId: string, paused: boolean) => call<void>("pause_run", { runId, paused }),
  setRunMode: (runId: string, mode: string) => call<void>("set_run_mode", { runId, mode }),
  reassignStep: (runId: string, stepId: string, assignment: Assignment) =>
    call<T.StepRow>("reassign_step", { runId, stepId, assignment }),
  answerApproval: (id: string, option: string) => call<void>("answer_approval", { id, option }),
  pendingApprovals: () => call<T.ApprovalRequest[]>("pending_approvals"),
  getRun: (runId: string) => call<T.RunDetail>("get_run", { runId }),
  listRuns: (workspaceId?: string | null, since?: number | null, limit?: number) =>
    call<T.RunRow[]>("list_runs", { workspaceId: workspaceId ?? null, since: since ?? null, limit: limit ?? 200 }),
  stepLogs: (runId: string, stepId: string, stepIdx: number, afterSeq: number) =>
    call<T.LogLine[]>("step_logs", { runId, stepId, stepIdx, afterSeq }),
  rollbackStep: (runId: string, idx: number) => call<number>("rollback_step", { runId, idx }),
  acceptRun: (runId: string, merge: boolean, restoreStash: boolean) =>
    call<string>("accept_run", { runId, merge, restoreStash }),
  exportRun: (runId: string, path: string) => call<void>("export_run", { runId, path }),
  stopEverything: () => call<string>("stop_everything"),

  libraryList: (workspaceId: string | null) => call<T.LibraryView>("library_list", { workspaceId }),
  libraryCreate: (kind: T.LibraryKind, scope: T.LibraryScope, displayName: string, workspaceId: string | null) =>
    call<T.LibraryItem>("library_create", { kind, scope, displayName, workspaceId }),
  librarySave: (path: string, raw: string, workspaceId: string | null) =>
    call<T.SyncReport | null>("library_save", { path, raw, workspaceId }),
  libraryRename: (path: string, displayName: string, workspaceId: string | null) =>
    call<void>("library_rename", { path, displayName, workspaceId }),
  libraryToggle: (path: string, enabled: boolean, workspaceId: string | null) =>
    call<void>("library_toggle", { path, enabled, workspaceId }),
  libraryReferences: (path: string, workspaceId: string | null) =>
    call<string[]>("library_references", { path, workspaceId }),
  libraryDelete: (path: string, workspaceId: string | null) => call<void>("library_delete", { path, workspaceId }),
  libraryDuplicate: (path: string, workspaceId: string | null) =>
    call<string>("library_duplicate", { path, workspaceId }),
  libraryMove: (path: string, to: T.LibraryScope, keepOriginal: boolean, workspaceId: string | null) =>
    call<string>("library_move", { path, to, keepOriginal, workspaceId }),
  libraryPreview: (path: string, workspaceId: string | null) =>
    call<T.Preview[]>("library_preview", { path, workspaceId }),
  librarySync: (workspaceId: string | null) => call<T.SyncReport>("library_sync", { workspaceId }),
  libraryImport: (workspaceId: string, scope: T.LibraryScope) =>
    call<number>("library_import", { workspaceId, scope }),
  libraryImportEdit: (workspaceId: string, rel: string) =>
    call<string | null>("library_import_edit", { workspaceId, rel }),
  saveProfile: (id: string | null, displayName: string, itemIds: string[]) =>
    call<string>("save_profile", { id, displayName, itemIds }),
  deleteProfile: (id: string) => call<void>("delete_profile", { id }),

  agents: (refresh = false, auth = false) => call<T.AgentInfo[]>("agents", { refresh, auth }),
  agentSet: (id: string, displayName?: string | null, enabled?: boolean | null) =>
    call<void>("agent_set", { id, displayName: displayName ?? null, enabled: enabled ?? null }),
  installAgent: (id: string, option: number) => call<string>("install_agent", { id, option }),
  uninstallAgent: (id: string) => call<string>("uninstall_agent", { id }),
  installRuntime: (id: string, option: number) => call<string>("install_runtime", { id, option }),
  cancelJob: (id: string) => call<void>("cancel_job", { id }),
  openLogin: (id: string) => call<void>("open_login", { id }),

  localModels: () => call<T.LocalModel[]>("local_models"),
  hardwareCheck: () => call<T.HardwareCheck>("hardware_check"),
  modelSuggestions: () => call<T.Suggestion[]>("model_suggestions"),
  ollamaStatus: () => call<T.OllamaStatus>("ollama_status"),
  ollamaStart: () => call<T.OllamaStatus>("ollama_start"),
  ollamaStop: () => call<T.OllamaStatus>("ollama_stop"),
  pullModel: (name: string) => call<void>("pull_model", { name }),
  cancelPull: (name: string) => call<void>("cancel_pull", { name }),
  deleteModel: (name: string) => call<number>("delete_model", { name }),
  unloadModel: (name: string) => call<number>("unload_model", { name }),
  setModelFolder: (path: string | null) => call<T.OllamaStatus>("set_model_folder", { path }),
  moveModels: (to: string) => call<void>("move_models", { to }),
  importGguf: (path: string, name: string) => call<void>("import_gguf", { path, name }),

  providersList: () => call<T.ProviderView[]>("providers_list"),
  connectProvider: (p: { id: string; display_name: string; type: T.ProviderType; base_url: string; key?: string | null }) =>
    call<T.CloudModel[]>("connect_provider", { p }),
  providerModels: (id: string) => call<T.CloudModel[]>("provider_models", { id }),
  updateProvider: (row: Omit<T.ProviderView, "key_hint">) => call<void>("update_provider", { row }),
  disconnectProvider: (id: string) => call<void>("disconnect_provider", { id }),
  connectModel: (m: {
    agent_id: string;
    provider_id: string;
    name: string;
    display_name?: string | null;
    price_in_per_m?: number | null;
    price_out_per_m?: number | null;
    ctx_len?: number | null;
    tool_calling?: boolean | null;
    target: "pool_front" | "pool_back" | "budget_planner";
  }) => call<T.Settings>("connect_model", { m }),
  smokeTest: (entry: T.ExecutorEntry) => call<{ ok: boolean; message: string }>("smoke_test", { entry }),
  modelsList: () => call<T.ModelRow[]>("models_list"),
  updateModel: (row: T.ModelRow) => call<T.Settings>("update_model", { row }),

  resourcesView: (open: boolean) => call<T.Snapshot>("resources_view", { open }),
  resourceSnapshot: () => call<T.Snapshot>("resource_snapshot"),
  whySlow: () => call<T.WhySlow>("why_slow"),
  metricsSince: (since: number) => call<T.MetricRow[]>("metrics_since", { since }),
  outcomeStats: (workspaceId?: string | null) => call<T.OutcomeStat[]>("outcome_stats", { workspaceId: workspaceId ?? null }),
  installCliCommand: () => call<string>("install_cli_command"),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
