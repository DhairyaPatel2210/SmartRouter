// Minimal stand-in for the Tauri runtime so the UI can render in a browser.
(() => {
  const now = Date.now();
  const callbacks = new Map();
  let nextId = 1;
  const listeners = {};
  const modes = [
    { id: "cost", display_name: "Cost", builtin: true, description: "Cheap executors for every step.", high: "cheap", low: "cheap", trivial: "cheap", unclear_as: "low", review: false, escalation: "approval", attempts_per_executor: 2, short_plan: true },
    { id: "balanced", display_name: "Balanced", builtin: true, description: "Paid for hard steps.", high: "paid", low: "cheap", trivial: "cheap", unclear_as: "high", review: true, escalation: "auto", attempts_per_executor: 2, short_plan: false },
    { id: "intelligent", display_name: "Intelligent", builtin: true, description: "Paid for everything but chores.", high: "paid", low: "paid", trivial: "cheap", unclear_as: "high", review: true, escalation: "auto", attempts_per_executor: 2, short_plan: false },
  ];
  const local = { provider_id: "ollama", provider_type: "ollama", name: "qwen2.5-coder:7b", display_name: "Qwen2.5 Coder 7B", tier: "local", mem_needed_gb: 6.0, ctx_len: 16384 };
  const cloud = { provider_id: "deepseek", provider_type: "openai_compatible", name: "deepseek-chat", display_name: "DeepSeek Chat", tier: "cheap_cloud", price_in_per_m: 0.27, price_out_per_m: 1.1 };
  const settings = {
    onboarded: !location.hash.includes("onboarding"), default_mode: "balanced", modes, planner_agent: "claude", budget_planning: false, budget_planner: null,
    executor_pool: [{ agent_id: "opencode", model: local, enabled: true }, { agent_id: "opencode", model: cloud, enabled: true }],
    approve_before_paid: false, approve_before_cloud: false, run_budget: {}, agent_prices: { claude: { in_per_m: 3, out_per_m: 15 } }, cloud_notice_ack: [],
    memory_budget_pct: null, idle_unload_minutes: 5, prefer_cloud_on_battery: true, prefer_cloud_when_hot: true, lower_priority: true, global_concurrency: 2,
    step_timeout_minutes: 15, model_folder: null, manage_ollama: true, ollama_always_on: false, menu_bar: true, theme: location.hash.includes("light") ? "light" : "dark", log_ring_lines: 2000, notifications: true,
  };
  const agent = (id, name, family, installed, extra = {}) => ({ id, display_name: name, family, installed, version: installed ? "1.2.3" : null, path: installed ? `/usr/local/bin/${id}` : null, auth: installed ? { state: "ok" } : null, providers: family === "open_source" ? ["ollama", "openai_compatible", "openrouter"] : [], login_command: family === "paid" ? `${id} login` : null, enabled: true, tagline: `${name} tagline`, docs: "https://example.com", known_good_version: "1.2.3", ...extra });
  const agents = [agent("cursor", "Cursor Agent", "paid", false), agent("claude", "Claude Code", "paid", true), agent("codex", "Codex CLI", "paid", false), agent("copilot", "GitHub Copilot CLI", "paid", false), agent("opencode", "OpenCode", "open_source", true), agent("aider", "Aider", "open_source", false), agent("goose", "Goose", "open_source", false)];
  const ws = { id: "w1", path: "/Users/me/code/acme-api", display_name: "acme-api", is_git: true, default_mode: null, library_profile_id: null, local_only: false, last_opened_at: now, pinned: true, settings: { check_commands: null, run_budget: null, approve_before_paid: null, approve_before_cloud: null, dirty_strategy: null, commit_generated: false, import_offered: false } };
  const info = { path: ws.path, name: ws.display_name, exists: true, is_git: true, branch: "main", dirty_files: ["src/app.ts", "README.md"], dirty_count: 2, writable: true, unsafe_root: false, detected_checks: ["npm run lint", "npm test"], importable: [".cursor/rules (3)", "AGENTS.md"] };
  const step = (idx, title, kind, cls, tier, agent_id, status, extra = {}) => ({ id: `s${idx}-${kind}`, run_id: "r1", idx, title, class: cls, kind, agent_id, model_id: tier === "premium" ? null : `${tier === "local" ? "ollama/qwen2.5-coder:7b" : "deepseek/deepseek-chat"}`, tier, library_agent_id: null, status, attempts: status === "pending" ? 0 : 1, escalated: false, route_reason: tier === "premium" ? "Balanced: high step → paid; hard rule: auth code" : "Balanced: low step → cheap", tokens_in: status === "pending" ? 0 : 3400, tokens_out: status === "pending" ? 0 : 820, tokens_estimated: false, cost_usd: tier === "premium" && status !== "pending" ? 0.072 : tier === "cheap_cloud" ? 0.0019 : 0, paid_equiv_usd: status === "pending" ? 0 : 0.0225, commit_ref: status === "passed" ? "abc123" : null, started_at: now - 60000, ended_at: null, detail: { executor: tier === "premium" ? "Claude Code" : tier === "local" ? "OpenCode · Qwen2.5 Coder 7B" : "OpenCode · DeepSeek Chat", summary: status === "passed" ? "Added input validation and 6 tests." : undefined, changed: status === "passed" ? ["src/validate.ts", "test/validate.test.ts"] : undefined }, ...extra });
  const steps = [
    step(0, "Plan the work", "plan", "high", "premium", "claude", "passed"),
    step(1, "Add request validation helpers", "execute", "low", "local", "opencode", "passed"),
    step(2, "Fix the auth token refresh race", "execute", "high", "premium", "claude", "running"),
    step(3, "Write tests for the validators", "execute", "low", "local", "opencode", "pending", { library_agent_id: "test-writer" }),
    step(4, "Update the README", "execute", "trivial", "cheap_cloud", "opencode", "pending"),
  ];
  const run = { id: "r1", workspace_id: "w1", goal: "Add input validation to the API with tests", mode: "balanced", status: "running", started_at: now - 184000, ended_at: null, est_cost_usd: 0.074, peak_mem_mb: 6900, library_snapshot_hash: "a1b2", budget_planning: false, branch: "orchestrator/1a2b3c4d", base_ref: "main", summary: {} };
  const snapshot = { ts: now, sys: { total_gb: 16, used_gb: 11.2, free_gb: 4.8, pressure: "normal", thermal: "nominal", power: "ac", low_power: false }, app: { label: "This app", owner: "app", pid: 1, rss_mb: 96, cpu_pct: 0.3, processes: 1 }, procs: [{ label: "Claude Code", owner: "s2", pid: 222, rss_mb: 310, cpu_pct: 12, processes: 3 }, { label: "Ollama (managed by this app)", owner: "ollama", pid: 333, rss_mb: 5900, cpu_pct: 4, processes: 2 }], models: [{ name: "qwen2.5-coder:7b", mem_mb: 5800, vram_mb: 5800, expires_at: new Date(now + 240000).toISOString(), loaded_by_app: true }], running: ["Claude Code (premium)"], cost_usd: 0.074, controlled_mb: 6210, sampling: true };
  const item = (id, kind, scope, name, raw) => ({ id, kind, scope, display_name: name, description: `${name} description`, enabled: true, path: `/lib/${kind}s/${id}.md`, body: raw, applies_to: [], always_on: kind === "rule", tier: kind === "agent" ? "cheap" : null, model: null, tools: [], skills: [], rules: [], updated_at: now, overridden: false, raw });
  const library = { items: [item("conventional-commits", "rule", "global", "Conventional commits", "---\ndisplayName: Conventional commits\nalwaysOn: true\n---\n\nWrite commit messages as `type(scope): summary`."), item("write-migration", "skill", "global", "Write migration", "---\nname: write-migration\ndescription: Use for schema changes.\n---\n\n# Write migration\n\n1. Find the migrations folder."), item("test-writer", "agent", "workspace", "Test writer", "---\ndisplayName: Test writer\ntier: cheap\n---\n\nYou write focused unit tests.")], profiles: [], targets: ["claude", "opencode"], conflicts: [], support: {}, import_candidates: [".cursor/rules (3)"], global_root: "/lib" };
  const logs = Array.from({ length: 40 }, (_, i) => ({ seq: i + 1, ts: now, kind: ["route", "out", "tool", "edit", "out", "check"][i % 6], text: ["Attempt 1 on Claude Code: Balanced: high step → paid", "Reading src/auth/refresh.ts to understand the token flow", "Read src/auth/refresh.ts", "src/auth/refresh.ts", "Added a mutex around refresh so concurrent requests share one in-flight call", "✓ `npm test` (4.2s)"][i % 6] }));
  const responses = {
    bootstrap: { brand: { productName: "Hybrid Agent Orchestrator", shortName: "Orchestrator", handoffDirName: ".orchestrator", cliName: "orch" }, settings, agents, recent: [ws, { ...ws, id: "w2", path: "/Users/me/code/web", display_name: "web", pinned: false }], active_runs: location.hash.includes("run") ? ["r1"] : [], approvals: [], data_dir: "/Users/me/Library/Application Support/orchestrator", catalog: { agents: [], runtimes: [{ id: "ollama", display_name: "Ollama", type: "ollama", endpoint: "", default: true, install: { macos: [{ label: "Homebrew", command: "brew install ollama" }] }, docs: "" }], models: [], providers: [{ id: "openrouter", display_name: "OpenRouter", type: "openrouter", base_url: "https://openrouter.ai/api/v1", signup_url: "", key_prefix: "sk-or-", notes: "Hundreds of models behind one key." }, { id: "deepseek", display_name: "DeepSeek", type: "openai_compatible", base_url: "", signup_url: "", key_prefix: "sk-", notes: "Very cheap, strong at code." }, { id: "gemini", display_name: "Google Gemini", type: "gemini", base_url: "", signup_url: "", key_prefix: "", notes: "Generous free tier." }] }, os: "macos", version: "0.1.0" },
    take_pending_open: null,
    open_workspace: { row: ws, info },
    inspect_workspace: { row: ws, info },
    recent_workspaces: [ws],
    resource_snapshot: snapshot,
    resources_view: snapshot,
    preflight: { fit: "tight", line: "Loads Qwen2.5 Coder 7B (~6.0 GB). You have 7.5 GB free, so ~1.5 GB after load: Tight.", model: "qwen2.5-coder:7b", mem_needed_gb: 6, free_gb: 7.5, budget_gb: 9.6, notes: ["On battery: cheap steps prefer cloud (Settings → Resources)."], sys: snapshot.sys, smaller: [{ agent_id: "opencode", model: { ...local, name: "qwen2.5-coder:3b", display_name: null }, enabled: true }], cloud: [{ agent_id: "opencode", model: cloud, enabled: true }], paid_agent: "Claude Code", pool_empty: false },
    list_runs: [{ ...run, status: "succeeded", ended_at: now - 3600000, started_at: now - 3800000, summary: { local_steps: 3, cheap_cloud_steps: 1, premium_steps: 2, steps_passed: 4, steps_total: 4, saved_usd: 0.31 } }, { ...run, id: "r2", goal: "Fix the flaky upload test", status: "failed", started_at: now - 86400000, ended_at: now - 86000000, summary: { local_steps: 2, premium_steps: 1 } }],
    get_run: { run, steps, active: true, paused: false, workspace: ws },
    step_logs: logs,
    library_list: library,
    library_preview: [{ cli: "claude", support: "native", text: "---\nname: test-writer\n---\n\nYou write focused unit tests.", installed: true }, { cli: "cursor", support: "converted", text: "...", installed: false }, { cli: "opencode", support: "native", text: "...", installed: true }],
    agents,
    providers_list: [{ id: "deepseek", display_name: "DeepSeek", type: "openai_compatible", base_url: "https://api.deepseek.com/v1", key_ref: "provider:deepseek", enabled: true, monthly_cap_usd: 10, key_hint: "••••9f2a" }],
    local_models: [{ name: "qwen2.5-coder:7b", size_gb: 4.68, loaded: true, mem_gb: 6.0, ctx: 16384, fit: "fits", catalog: { id: "qwen2.5-coder:7b", display_name: "Qwen2.5 Coder 7B" }, in_pool: true }, { name: "llama3.1:latest", size_gb: 4.9, loaded: false, mem_gb: 6.3, ctx: 16384, fit: "tight", catalog: null, in_pool: false }],
    hardware_check: { hw: { chip: "Apple M2 Pro", total_mem_gb: 16, cores: 10, apple_silicon: true, os: "macos" }, free_mem_gb: 7.5, budget_gb: 9.6, budget_pct: 60, model_folder: "/Users/me/.ollama/models", free_disk_gb: 212 },
    model_suggestions: [{ model: { id: "qwen2.5-coder:7b", display_name: "Qwen2.5 Coder 7B", params_b: 7.6, quant: "Q4_K_M", download_gb: 4.7, mem_gb: 6, ctx_max: 32768, tool_calling: true, runtime: "ollama", good_for: "Solid routine coding on 16 GB Macs" }, fit: "fits", ctx: 16384, mem_gb: 6, installed: true, best: true, disk_ok: true }, { model: { id: "qwen2.5-coder:3b", display_name: "Qwen2.5 Coder 3B", params_b: 3.1, quant: "Q4_K_M", download_gb: 1.9, mem_gb: 3.2, ctx_max: 32768, tool_calling: true, runtime: "ollama", good_for: "Small edits, tests, docs" }, fit: "fits", ctx: 16384, mem_gb: 3.2, installed: false, best: false, disk_ok: true }],
    ollama_status: { installed: true, running: true, managed_by_app: true, version: "0.24.0", endpoint: "http://127.0.0.1:11434", models_dir: "/Users/me/.ollama/models", folder_hint: null },
    detect_runtimes: [],
    scan: { agents, hardware: null, ollama: null, local_models: [], runtimes: [], suggestions: [], library_found: [], elapsed_ms: 640 },
    models_list: [],
    outcome_stats: [{ class: "low", tier: "local", model: "ollama/qwen2.5-coder:7b", budget_planning: false, first_try_passes: 8, total: 10 }, { class: "high", tier: "premium", model: "claude", budget_planning: false, first_try_passes: 5, total: 6 }],
    workflow_list: [],
    pending_approvals: [],
    metrics_since: [],
  };
  responses.scan.hardware = responses.hardware_check;
  responses.scan.ollama = responses.ollama_status;
  responses.scan.local_models = responses.local_models;
  responses.scan.suggestions = responses.model_suggestions;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback(cb) {
      const id = nextId++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    convertFileSrc: (p) => p,
    async invoke(cmd, args) {
      if (cmd === "plugin:event|listen") {
        (listeners[args.event] ||= []).push(args.handler);
        return args.handler;
      }
      if (cmd.startsWith("plugin:")) return null;
      if (cmd === "save_settings") return args.settings;
      const o = window.__mockOverrides || {};
      if (cmd in o) return structuredClone(typeof o[cmd] === "function" ? o[cmd](args) : o[cmd]);
      return cmd in responses ? structuredClone(responses[cmd]) : null;
    },
  };
  window.__emit = (event, payload) => (listeners[event] || []).forEach((h) => callbacks.get(h)?.({ event, id: 0, payload }));
})();
