// Mirrors the Rust types serialized by the core (snake_case JSON).

export type Tier = "local" | "cheap_cloud" | "premium";
export type StepClass = "high" | "low" | "trivial";
export type AgentFamily = "paid" | "open_source";
export type ProviderType =
  | "ollama"
  | "openai_compatible"
  | "openrouter"
  | "anthropic"
  | "openai"
  | "gemini"
  | "lmstudio"
  | "llamacpp"
  | "mlx"
  | "fake";
export type RunStatus = "pending" | "planning" | "running" | "paused" | "reviewing" | "succeeded" | "failed" | "cancelled";
export type StepStatus =
  | "pending"
  | "running"
  | "verifying"
  | "awaiting_approval"
  | "passed"
  | "failed"
  | "accepted"
  | "skipped"
  | "cancelled"
  | "rolled_back";
export type Fit = "fits" | "tight" | "wont_fit";
export type Exec = "paid" | "cheap";

export interface ModelRef {
  provider_id: string;
  provider_type: ProviderType;
  name: string;
  display_name?: string | null;
  base_url?: string | null;
  key_ref?: string | null;
  tier: Tier;
  mem_needed_gb?: number | null;
  ctx_len?: number | null;
  price_in_per_m?: number | null;
  price_out_per_m?: number | null;
}

export interface ExecutorEntry {
  agent_id: string;
  model: ModelRef;
  enabled: boolean;
}

export interface ModeDef {
  id: string;
  display_name: string;
  builtin: boolean;
  description: string;
  high: Exec;
  low: Exec;
  trivial: Exec;
  unclear_as: StepClass;
  review: boolean;
  escalation: "auto" | "approval" | "never";
  attempts_per_executor: number;
  short_plan: boolean;
}

export interface AgentPrice {
  in_per_m?: number | null;
  out_per_m?: number | null;
  per_request?: number | null;
}

export interface RunBudget {
  max_cost_usd?: number | null;
  max_steps?: number | null;
  max_paid_tokens?: number | null;
}

export interface Settings {
  onboarded: boolean;
  default_mode: string;
  modes: ModeDef[];
  planner_agent: string | null;
  budget_planning: boolean;
  budget_planner: ExecutorEntry | null;
  executor_pool: ExecutorEntry[];
  approve_before_paid: boolean;
  approve_before_cloud: boolean;
  run_budget: RunBudget;
  agent_prices: Record<string, AgentPrice>;
  cloud_notice_ack: string[];
  memory_budget_pct: number | null;
  idle_unload_minutes: number;
  prefer_cloud_on_battery: boolean;
  prefer_cloud_when_hot: boolean;
  lower_priority: boolean;
  global_concurrency: number;
  step_timeout_minutes: number;
  model_folder: string | null;
  manage_ollama: boolean;
  ollama_always_on: boolean;
  menu_bar: boolean;
  theme: "system" | "light" | "dark";
  log_ring_lines: number;
  notifications: boolean;
}

export type DirtyStrategy = "stash" | "commit" | "run_on_top";

export interface WorkspaceSettings {
  check_commands: string[] | null;
  run_budget: RunBudget | null;
  approve_before_paid: boolean | null;
  approve_before_cloud: boolean | null;
  dirty_strategy: DirtyStrategy | null;
  commit_generated: boolean;
  import_offered: boolean;
}

export interface WorkspaceRow {
  id: string;
  path: string;
  display_name: string;
  is_git: boolean;
  default_mode: string | null;
  library_profile_id: string | null;
  local_only: boolean;
  last_opened_at: number;
  pinned: boolean;
  settings: WorkspaceSettings;
}

export interface WorkspaceInfo {
  path: string;
  name: string;
  exists: boolean;
  is_git: boolean;
  branch: string | null;
  dirty_files: string[];
  dirty_count: number;
  writable: boolean;
  unsafe_root: boolean;
  detected_checks: string[];
  importable: string[];
}

export interface OpenedWorkspace {
  row: WorkspaceRow;
  info: WorkspaceInfo;
}

export interface RunSummary {
  steps_total?: number;
  steps_passed?: number;
  steps_failed?: number;
  premium_steps?: number;
  cheap_cloud_steps?: number;
  local_steps?: number;
  tokens_in?: number;
  tokens_out?: number;
  tokens_estimated?: boolean;
  cost_premium?: number;
  cost_cheap_cloud?: number;
  cost_local?: number;
  saved_usd?: number;
  escalations?: number;
  stashed?: string;
  base_commit?: string;
  review_verdict?: string;
  accepted?: boolean;
  error?: string;
}

export interface RunRow {
  id: string;
  workspace_id: string;
  goal: string;
  mode: string;
  status: RunStatus;
  started_at: number;
  ended_at: number | null;
  est_cost_usd: number;
  peak_mem_mb: number;
  library_snapshot_hash: string | null;
  budget_planning: boolean;
  branch: string | null;
  base_ref: string | null;
  summary: RunSummary;
}

export interface StepRow {
  id: string;
  run_id: string;
  idx: number;
  title: string;
  class: StepClass;
  kind: "plan" | "execute" | "review";
  agent_id: string | null;
  model_id: string | null;
  tier: Tier | null;
  library_agent_id: string | null;
  status: StepStatus;
  attempts: number;
  escalated: boolean;
  route_reason: string | null;
  tokens_in: number;
  tokens_out: number;
  tokens_estimated: boolean;
  cost_usd: number;
  paid_equiv_usd: number;
  commit_ref: string | null;
  started_at: number | null;
  ended_at: number | null;
  detail: {
    model?: ModelRef | null;
    executor?: string;
    class_reason?: string;
    files?: string[];
    check?: string | null;
    edits?: string[];
    changed?: string[];
    summary?: string;
    agent_error?: string;
    unverified?: boolean;
    verdict?: string;
    review?: string;
    tasks?: number;
  };
}

export interface RunDetail {
  run: RunRow;
  steps: StepRow[];
  active: boolean;
  paused: boolean;
  workspace: WorkspaceRow | null;
}

export interface LogLine {
  seq: number;
  ts: number;
  kind: "out" | "tool" | "edit" | "error" | "governor" | "check" | "route" | "info";
  text: string;
}

export interface ApprovalOption {
  id: string;
  label: string;
  primary: boolean;
  danger: boolean;
}

export interface ApprovalRequest {
  id: string;
  run_id: string;
  step_id: string | null;
  kind: string;
  title: string;
  body: string;
  options: ApprovalOption[];
}

export type AuthStatus = { state: "ok" } | { state: "missing"; hint: string } | { state: "unknown" };

export interface AgentInfo {
  id: string;
  display_name: string;
  family: AgentFamily;
  installed: boolean;
  version: string | null;
  path: string | null;
  auth: AuthStatus | null;
  providers: ProviderType[];
  login_command: string | null;
  enabled: boolean;
  tagline: string;
  docs: string;
  known_good_version: string;
}

export interface ProcUsage {
  label: string;
  owner: string;
  pid: number;
  rss_mb: number;
  cpu_pct: number;
  processes: number;
}

export interface ModelUsage {
  name: string;
  mem_mb: number;
  vram_mb: number;
  expires_at: string;
  loaded_by_app: boolean;
}

export type Pressure = "normal" | "warning" | "critical";
export type Thermal = "nominal" | "fair" | "serious" | "critical";
export type PowerSource = "ac" | "battery" | "unknown";

export interface Snapshot {
  ts: number;
  sys: {
    total_gb: number;
    used_gb: number;
    free_gb: number;
    pressure: Pressure | null;
    thermal: Thermal | null;
    power: PowerSource | null;
    low_power: boolean;
  };
  app: ProcUsage | null;
  procs: ProcUsage[];
  models: ModelUsage[];
  running: string[];
  cost_usd: number;
  controlled_mb: number;
  sampling: boolean;
}

export interface SysView {
  total_gb: number;
  free_gb: number;
  pressure: Pressure;
  thermal: Thermal;
  power: PowerSource;
  low_power: boolean;
}

export interface PreflightView {
  fit: Fit | null;
  line: string;
  model: string | null;
  mem_needed_gb: number | null;
  free_gb: number;
  budget_gb: number;
  notes: string[];
  sys: SysView;
  smaller: ExecutorEntry[];
  cloud: ExecutorEntry[];
  paid_agent: string | null;
  pool_empty: boolean;
}

export type LibraryKind = "rule" | "skill" | "agent";
export type LibraryScope = "global" | "workspace";
export type Support = "native" | "converted" | "fallback";

export interface LibraryItem {
  id: string;
  kind: LibraryKind;
  scope: LibraryScope;
  display_name: string;
  description: string;
  enabled: boolean;
  path: string;
  body: string;
  applies_to: string[];
  always_on: boolean;
  tier: string | null;
  model: string | null;
  tools: string[];
  skills: string[];
  rules: string[];
  updated_at: number;
  overridden: boolean;
  raw: string;
}

export interface LibraryProfile {
  id: string;
  display_name: string;
  item_ids: string[];
}

export interface Conflict {
  path: string;
  reason: "edited" | "exists";
}

export interface LibraryView {
  items: LibraryItem[];
  profiles: LibraryProfile[];
  targets: string[];
  conflicts: Conflict[];
  support: Record<string, Record<LibraryKind, Support>>;
  import_candidates: string[];
  global_root: string;
}

export interface SyncReport {
  written: string[];
  removed: string[];
  unchanged: number;
  conflicts: Conflict[];
}

export interface Preview {
  cli: string;
  support: Support;
  text: string;
  installed: boolean;
}

export interface InstallOption {
  label: string;
  command: string;
}

export interface CatalogAgent {
  id: string;
  display_name: string;
  family: AgentFamily;
  tagline: string;
  bin: string;
  install: Record<string, InstallOption[]>;
  uninstall: Record<string, string>;
  docs: string;
  known_good_version: string;
}

export interface CatalogRuntime {
  id: string;
  display_name: string;
  type: string;
  endpoint: string;
  default: boolean;
  install: Record<string, InstallOption[]>;
  docs: string;
}

export interface CatalogModel {
  id: string;
  display_name: string;
  params_b: number;
  quant: string;
  download_gb: number;
  mem_gb: number;
  ctx_max: number;
  tool_calling: boolean;
  runtime: string;
  good_for: string;
}

export interface CatalogProvider {
  id: string;
  display_name: string;
  type: ProviderType;
  base_url: string;
  signup_url: string;
  key_prefix: string;
  notes: string;
}

export interface Catalog {
  agents: CatalogAgent[];
  runtimes: CatalogRuntime[];
  models: CatalogModel[];
  providers: CatalogProvider[];
}

export interface Brand {
  productName: string;
  shortName: string;
  handoffDirName: string;
  cliName: string;
}

export interface Bootstrap {
  brand: Brand;
  settings: Settings;
  agents: AgentInfo[];
  recent: WorkspaceRow[];
  active_runs: string[];
  approvals: ApprovalRequest[];
  data_dir: string;
  catalog: Catalog;
  os: string;
  version: string;
}

export interface Hardware {
  chip: string;
  total_mem_gb: number;
  cores: number;
  apple_silicon: boolean;
  os: string;
}

export interface HardwareCheck {
  hw: Hardware;
  free_mem_gb: number;
  budget_gb: number;
  budget_pct: number;
  model_folder: string;
  free_disk_gb: number | null;
}

export interface Suggestion {
  model: CatalogModel;
  fit: Fit;
  ctx: number;
  mem_gb: number;
  installed: boolean;
  best: boolean;
  disk_ok: boolean;
}

export interface OllamaStatus {
  installed: boolean;
  running: boolean;
  managed_by_app: boolean;
  version: string | null;
  endpoint: string;
  models_dir: string;
  folder_hint: string | null;
}

export interface LocalModel {
  name: string;
  size_gb: number;
  loaded: boolean;
  mem_gb: number;
  ctx: number;
  fit: Fit;
  catalog: CatalogModel | null;
  in_pool: boolean;
}

export interface DetectedRuntime {
  kind: ProviderType;
  endpoint: string;
  running: boolean;
  models: string[];
}

export interface ScanResult {
  agents: AgentInfo[];
  hardware: HardwareCheck;
  ollama: OllamaStatus;
  local_models: LocalModel[];
  runtimes: DetectedRuntime[];
  suggestions: Suggestion[];
  library_found: string[];
  elapsed_ms: number;
}

export interface ProviderView {
  id: string;
  display_name: string;
  type: ProviderType;
  base_url: string | null;
  key_ref: string | null;
  enabled: boolean;
  monthly_cap_usd: number | null;
  key_hint: string | null;
}

export interface CloudModel {
  id: string;
  name: string;
  ctx_len: number | null;
  price_in_per_m: number | null;
  price_out_per_m: number | null;
  tool_calling: boolean | null;
}

export interface ModelRow {
  id: string;
  display_name: string;
  provider_id: string;
  runtime_id: string | null;
  name: string;
  tier: Tier;
  size_gb: number | null;
  mem_needed_gb: number | null;
  quant: string | null;
  ctx_len: number | null;
  tool_calling: boolean | null;
  price_in_per_m: number | null;
  price_out_per_m: number | null;
  path: string | null;
  installed: boolean;
}

export interface WhySlow {
  snapshot: Snapshot;
  others: { name: string; pid: number; rss_mb: number; cpu_pct: number; controlled: boolean }[];
  advice: string[];
}

export interface MetricRow {
  ts: number;
  target: string;
  rss_mb: number;
  cpu_pct: number;
  vram_mb: number | null;
}

export interface OutcomeStat {
  class: string;
  tier: string;
  model: string;
  budget_planning: boolean;
  first_try_passes: number;
  total: number;
}

export type UiEvent =
  | { type: "run"; run: RunRow }
  | { type: "step"; step: StepRow }
  | { type: "log"; run_id: string; step_id: string; line: LogLine }
  | { type: "notice"; level: string; text: string; run_id: string | null }
  | { type: "approval"; request: ApprovalRequest }
  | { type: "approval_done"; id: string }
  | { type: "resources"; snapshot: Snapshot }
  | { type: "install"; id: string; line: string; done: boolean | null; progress: number | null }
  | { type: "pull"; model: string; status: string; completed: number; total: number; done: boolean | null; error: string | null }
  | { type: "move"; completed: number; total: number; done: boolean | null; error: string | null }
  | { type: "library"; workspace: string | null }
  | { type: "agents" };
