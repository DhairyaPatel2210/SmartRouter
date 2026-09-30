# Hybrid Agent Orchestrator — Build Spec

Sep 30, 2026 · @Dhruv · rev 3

> **Rev 3 changes:** efficiency on the Mac is now the top priority, with hard performance budgets and a resource governor that keeps the user informed; the catalog is about agents (paid CLIs and open-source CLIs), not a model list; models download into the Ollama directory or a folder the user chooses, with no duplicate downloads; planning without a paid agent ships in v1 (off by default).
>
> **Rev 2 changes:** shared Library of skills, agents and rules synced to every installed agent CLI; product name and all item names are renameable from one place; workspace (directory) picker; a "cheap cloud" tier so the app works with no local model.

## Overview

A native-feeling Mac desktop app that sends high-thinking coding work to a paid coding agent (Cursor Agent, Claude Code, Codex, Copilot) and routine work to an open-source coding agent (OpenCode first) running on a downloaded local model, or on a cheap cloud model when the user has no local model. The user picks a workspace and a mode, sees every handoff, and can override any routing decision.

The app itself must be light: it orchestrates heavy tools, so it must never be one of them. It always shows the user what is using their Mac's memory, CPU and battery, and why.

**Target users:** developers on Apple Silicon Macs who already use Cursor, Claude Code, Codex or Copilot and want to spend less, by running routine work on local models or cheap API models.

**Platform priority:** macOS on Apple Silicon first. Windows and Linux build in CI but are not tuned in v1.

**Core promises, in priority order:**

1. **Efficient and transparent:** the app is fast to open, near-idle when idle, and never lets local models overload the Mac without warning. The user always sees what is running, what it uses, and has one button to stop it all.
2. Route each task to the cheapest agent that can do it well, under one of three modes: Cost, Balanced, Intelligent.
3. Work in any directory the user chooses, like any other agentic app.
4. Work with or without a local model: local and cheap-cloud models are interchangeable behind the open-source agent.
5. One shared Library of skills, agents and rules that every installed agent CLI uses, written once and synced into each tool's native format.
6. Keep context across agents through shared files in the workspace, not hidden session state.
7. Install coding agents and open-source models in one click, sized to the user's hardware, stored where the user wants.
8. Onboarding in under 2 minutes, from launch to first routed task.
9. Every name is editable: the product name, and the display name of every skill, agent, rule, workflow and mode.

**Non-goals for v1:** hosting models ourselves, a custom LLM, our own coding agent, team or multi-user features, IDE plugins, a Library marketplace. (Calling third-party cloud model APIs with the user's own key is in scope.)

## Performance and resource efficiency (top priority)

Efficiency beats features. If a feature can't meet these budgets, it waits or ships behind a toggle.

**App budgets (measured on the baseline Mac: M1, 8 GB RAM, release build):**

| Metric | Budget |
| --- | --- |
| Cold start to interactive Home screen | < 1 s |
| App memory at idle (core + webview, no run) | < 150 MB |
| App CPU at idle, window open | < 0.5% |
| App CPU at idle, window hidden or minimised | ~0% (no timers except the governor's cheap pressure listener) |
| Core overhead during a run (excluding agents and models) | < 3% CPU, < 250 MB |
| UI frame time while logs stream | 60 fps; no frame over 50 ms |
| Installed app size | < 25 MB |
| App-caused disk writes during a run | Batched; no write per log line |

**How the app stays light (rules for the builder):**

- Native arm64 build; release profile with LTO; no Electron-style bundled runtimes.
- Event-driven, not polling: agent output is read from process pipes; Ollama status is checked on demand and on events. Metrics sampling runs only while a run is active or the Resource view is open.
- `sysinfo` refreshes only the tracked process trees, never a full system scan per tick.
- Logs: a bounded ring buffer in memory per step (default 2,000 lines); full logs stream to a file on disk. The UI gets events in batches (at most every 50 ms) and renders them in virtualised lists.
- SQLite in WAL mode with batched inserts (flush every 250 ms or 200 events).
- Heavy UI parts (charts, CodeMirror, React Flow) load only when their screen opens.
- Nothing runs after the app quits unless the user opts in. Services the app started (such as Ollama) stop on quit; services the user already ran are left alone.

**CI performance gate:** a `perf` job runs on macOS arm64 each PR and fails if startup, idle memory, or bundle size exceed the budgets by more than 10%. Results are appended to `docs/perf.md`.

### Resource governor

The `governor/` module owns every decision that affects the Mac's memory, CPU, heat and battery. It keeps the Mac responsive and tells the user, in plain language, what it did and why.

**Before a run (pre-flight):**

- Estimate the run's footprint: which local model will load, its memory (model size + context), and free memory now.
- Show a one-line summary with a Fits / Tight / Won't fit badge, e.g. "Loads Qwen Coder 14B (~9.5 GB). You have 11 GB free, so ~1.5 GB after load: Tight."
- If Tight or Won't fit, offer: a smaller model, a cheap cloud model for this run, or proceed anyway.

**During a run:**

- **Memory budget:** local models may use at most a set share of RAM (default 60% of unified memory on Macs with ≤ 16 GB, 70% above). The governor won't load a model that breaks the budget. It downgrades to a smaller model or falls through to cloud, per the executor pool, and says so.
- **One local model at a time** by default. When the app manages Ollama it sets the max-loaded-models and keep-alive settings to match (verify the current env var names). Idle models unload after 5 minutes (configurable), and after the run ends if nothing is queued.
- **Memory pressure:** listen to macOS memory-pressure events (dispatch source, no polling). Warning: show a banner and offer to unload models. Critical: pause the local step, unload the model, and offer to continue on cloud or wait.
- **Heat and battery:** read the macOS thermal state and Low Power Mode / power source. At serious thermal state, or on battery with "Prefer cloud on battery" on (default on), route new cheap steps to cloud if the pool has one; otherwise ask. Always say why.
- **Keep the Mac responsive:** agent CLIs and the app-managed model server run at a lower scheduling priority (macOS utility QoS / `nice`) so the user's editor and browser stay smooth. Configurable.
- **Concurrency:** at most one run using a local model at once on Macs with ≤ 16 GB; runs on cloud executors can run alongside it up to the global cap (default 2).

**Always visible (never leave the user clueless):**

- **Status bar** at the bottom of every screen: what's running (agent + model), memory used by models and agents vs free, CPU, and estimated cost so far. Click to open the Resource view.
- **Menu bar item** (optional, on by default): the same summary plus a **Stop everything** button that cancels all runs, kills every agent process tree, and unloads models the app loaded.
- **Plain-language reasons:** every governor action writes a short event such as "Paused step 3: memory pressure critical. Qwen 14B unloaded (9.1 GB freed)." These appear in the run log and as a macOS notification if the window is hidden.
- **"Why is my Mac slow?"** button in the Resource view: lists the top memory and CPU users that the app controls, with Unload / Stop buttons, and notes any heavy processes it doesn't control.

## Naming and branding

The product name is not final (see Open questions), so it must be changeable in one place without touching code.

- **Single source:** `brand.json` at the repo root holds `productName`, `shortName`, `bundleId`, `dataDirName`, `handoffDirName`, `cliName`, and tagline. Default values:

  ```json
  {
    "productName": "Hybrid Agent Orchestrator",
    "shortName": "Orchestrator",
    "bundleId": "dev.orchestrator.app",
    "dataDirName": "orchestrator",
    "handoffDirName": ".orchestrator",
    "cliName": "orch"
  }
  ```

- A build step (`scripts/apply-brand`) writes these into `tauri.conf.json`, `package.json`, window titles and generated constants for Rust (`brand.rs`) and TypeScript (`brand.ts`). Code never hard-codes the name; UI copy uses `{brand.productName}`.
- In this spec, "the app", `<data>` (the per-user data directory, e.g. `~/Library/Application Support/<dataDirName>`) and `.orchestrator/` refer to these values.
- Renaming `handoffDirName` after release comes with a migration that moves the old folder.
- **User-facing names:** every skill, agent, rule, workflow, mode and connected model has a stable internal `id` (slug, never shown or changed) and a `displayName` the user can rename inline (double-click or F2) anywhere it appears. References always use `id`, so a rename never breaks anything.

## Instructions for Claude (the builder)

Claude Code builds this project milestone by milestone from this spec. Export this doc as Markdown to `docs/SPEC.md` in the repo and read it at the start of every session.

**Working rules:**

1. Build one milestone at a time, in order. Do not start the next until every acceptance criterion of the current one passes.
2. Before coding a milestone, write `docs/plans/M<n>.md`: files to create, interfaces, test plan, and the expected effect on the performance budgets. Stop and ask the user to confirm it.
3. Keep `AGENTS.md` at the repo root current: stack, commands (dev, test, lint, build, perf), conventions, and decisions made.
4. Log every non-obvious decision in `docs/DECISIONS.md` with date, choice, and reason.
5. Write tests with each feature: Rust unit tests for the core, Vitest for the UI, and fake-CLI integration tests for adapters.
6. Never call a real paid agent or a real cloud model API in automated tests. Use the fake adapter and the fake provider (see Agent adapter layer).
7. Before using any external CLI flag, output format, env var, or native config location (skills, agents, rules folders, model directories), verify it against the tool's current `--help` or docs. These change often; record the verified version in `docs/DECISIONS.md`.
8. Ask the user when a choice affects cost, security, or user data. Otherwise pick the simplest option and log it.
9. Never hard-code the product name; read it from `brand.json`-generated constants.
10. Efficiency first: no new always-on timer, background thread, or dependency over 1 MB without a note in `docs/DECISIONS.md` and a passing perf job.

**Definition of done for any task:** code compiles, tests, lint and the perf job pass, the feature is visible in the UI, and `AGENTS.md` is updated if commands or structure changed.

## Tech stack and repo structure

Tauri 2 desktop app: a Rust core that owns processes, routing, the Library, the resource governor and metrics, and a React UI that only renders and sends commands.

| Layer | Choice | Why |
| --- | --- | --- |
| Shell | Tauri 2 (uses the system WebKit on macOS) | Small binary, low memory, native process control, native folder picker, menu bar item |
| Core | Rust, tokio | Spawn and stream CLI processes with low overhead |
| macOS integration | `objc2` bindings (or a small Swift helper if needed) | Memory-pressure events, thermal state, Low Power Mode, QoS, notifications |
| System metrics | `sysinfo` crate (targeted refresh) | Per-process RSS and CPU, total RAM |
| Storage | SQLite via `sqlx`, WAL mode | Runs, steps, events, settings; local only |
| Secrets | OS keychain via `keyring` crate | Cloud API keys; never in SQLite or files |
| Library format | Markdown + YAML frontmatter | Human-editable, diffable, matches the open Agent Skills `SKILL.md` format |
| UI | React + TypeScript + Vite, route-level code splitting | Fast start; heavy screens load on demand |
| Styling | Tailwind + shadcn/ui | Consistent components, small CSS |
| Lists | TanStack Virtual | Virtualised logs and history |
| Editor | CodeMirror 6 (lazy-loaded) | Editing skills, agents and rules in-app |
| Workflow canvas | React Flow (lazy-loaded, M5) | Node editor for the v2 workflow builder |
| Charts | Recharts (lazy-loaded) | Telemetry and memory graphs |
| State | Zustand + TanStack Query | Simple client state, cached core queries |
| Tests | cargo test, Vitest, Playwright, perf script | Core, UI, end-to-end, budgets |

**Repo layout:**

```
/brand.json             product name and identifiers (single source)
/AGENTS.md              rules and commands for any coding agent
/docs/SPEC.md           this spec
/docs/DECISIONS.md      decision log
/docs/perf.md           performance results per PR
/docs/plans/            one plan per milestone
/catalog/               agents.json, runtimes.json, models.json, providers.json, library-starter/
/scripts/apply-brand    writes brand.json into configs and constants
/scripts/perf           startup, idle memory and bundle size checks
/src-tauri/src/
    adapters/           one module per agent CLI + fake adapter
    providers/          model backends: ollama, openai-compatible (LM Studio, llama.cpp, MLX, cloud), fake
    governor/           memory budget, pressure/thermal/battery listeners, model load/unload, QoS
    router/             modes, tiers, rules, escalation
    workspace/          directory picker backend, recent list, git detection, scope guard
    library/            skills/agents/rules store, resolver, per-CLI sync writers
    handoff/            plan and task file writer and reader
    verify/             test/lint gate
    installer/          agent, runtime and model installers, hardware check, model directory
    telemetry/          event bus, metrics sampler, cost estimator
    macos/              native bindings (pressure, thermal, power, notifications, menu bar)
    db/                 SQLite schema and queries
    brand.rs            generated from brand.json
    commands.rs         Tauri commands exposed to the UI
/src/                   React app
    onboarding/ home/ runs/ library/ workflow/ telemetry/ settings/ statusbar/
/tests/fake-cli/        scripted fake agent for integration tests
/tests/fake-provider/   local HTTP server imitating an OpenAI-compatible API
```

## Architecture and data model

The UI never talks to an agent directly: it sends a goal and a workspace to the Rust core. The governor checks resources, the router splits the goal into steps, and the Library is synced into the workspace. Each step then runs through an adapter with a chosen model. Every event lands in SQLite and streams back to the UI in batches.

&#91;embedded content: architecture · UI, Rust core, adapters, repo\]

Only the core spawns agents; the workspace's files are the shared context every agent reads.

**Run lifecycle:**

1. User picks a workspace (directory), submits a goal, and chooses a mode.
2. The core checks the workspace (git state, write access) and the governor runs pre-flight (memory, thermal, battery).
3. The core syncs the active Library into the workspace (see Shared Library).
4. The router sends a planning step to the planner (the paid agent, or a cheap-cloud model with budget planning on), which writes `.orchestrator/plan.md` as a numbered task list.
5. The core parses the plan into steps and classifies each one (see Router).
6. Each step runs on its assigned agent + model, with the skills and rules that apply to it, then goes through the verification gate. The governor loads and unloads models as needed.
7. Pass: next step. Fail: retry or escalate per the mode's rules.
8. Optional final review in Balanced and Intelligent modes.

**Core tables (SQLite):**

| Table | Key fields |
| --- | --- |
| `agents` | id, display\_name, kind (paid/open\_source), cli, version, install\_path, auth\_ok, enabled, supports\_local\_models |
| `runtimes` | id, display\_name, type (ollama/lmstudio/llamacpp/mlx), endpoint, models\_dir, managed\_by\_app, running |
| `providers` | id, display\_name, type (ollama/openrouter/openai\_compatible/anthropic/…), base\_url, key\_ref (keychain entry name), enabled |
| `models` | id, display\_name, provider\_id, runtime\_id, name, tier (local/cheap\_cloud/premium), size\_gb, mem\_needed\_gb, quant, ctx\_len, tool\_calling, price\_in\_per\_m, price\_out\_per\_m, path, installed |
| `workspaces` | id, path, display\_name, is\_git, default\_mode, library\_profile\_id, local\_only, last\_opened\_at, pinned |
| `library_items` | id, kind (skill/agent/rule), display\_name, scope (global/workspace), source\_path, enabled, targets\_json, updated\_at |
| `library_profiles` | id, display\_name, item\_ids\_json |
| `runs` | id, workspace\_id, goal, mode, status, started\_at, ended\_at, est\_cost\_usd, peak\_mem\_mb, library\_snapshot\_hash |
| `steps` | id, run\_id, index, title, class (high/low/trivial), agent\_id, model\_id, library\_agent\_id, status, attempts, escalated, route\_reason |
| `events` | id, step\_id, ts, type (stdout/tool\_call/file\_edit/tokens/error/governor), payload\_json |
| `metrics` | ts, target (agent or model), rss\_mb, cpu\_pct, vram\_mb |
| `settings` | key, value\_json |

`events` stores normalized events only; raw stdout goes to log files under `<data>/logs/<run-id>/`. Metrics older than 30 days are pruned automatically.

All data stays on the user's machine, except the prompts the user's chosen agents and cloud models send to their own providers. Telemetry means local observability, not sending data anywhere.

## Workspaces

The user chooses the directory the agents work in, the way Cursor, Claude Code or any other agentic app does.

- **Pick:** Home has a workspace switcher: "Open folder…" (native folder dialog), drag-and-drop a folder onto the window, a recent list (last 10, pinnable), and a `cliName <path>` shell command that opens the app on that folder.
- **Per-workspace settings:** default mode, check commands, Library profile, budget, approval rules and "local only" are stored per workspace and override global defaults.
- **Git handling:**
  - Git repo with a clean tree: work on `orchestrator/<run-id>` with one commit per step (as in Verification).
  - Git repo with uncommitted changes: offer to stash, commit, or run on top (the user chooses; default is stash).
  - Not a git repo: offer "Initialise git" (recommended, enables per-step rollback) or "Run without git". Without git, the core snapshots each step's touched files under `<data>/snapshots/<run-id>/` so rollback still works. Snapshots are pruned when the run is accepted.
- **Scope guard:** all agents are started with the workspace as their working directory. The core watches file edits reported by the agents and warns if a step writes outside the workspace; in Cost mode it also pauses. Where a CLI supports a directory allow-list or sandbox flag, the adapter sets it to the workspace.
- **Refuse unsafe roots:** the home directory, `/`, and system folders need an explicit extra confirmation.
- **Multiple workspaces:** one active run per workspace; concurrency across workspaces follows the governor's rules.

## Shared Library: skills, agents and rules

Users write a skill, agent or rule once, and every installed agent CLI (Cursor Agent, Claude Code, Codex, Copilot, and the open-source agents) gets it in its own native format. This keeps behaviour consistent no matter which agent the router picks.

**Item kinds:**

| Kind | What it is | Canonical file |
| --- | --- | --- |
| Rule | Always-on or file-scoped instructions (style, conventions, "never touch X") | `rules/<id>.md` with frontmatter `displayName`, `appliesTo` (globs, optional), `alwaysOn` |
| Skill | A reusable capability loaded on demand: instructions plus optional scripts and reference files | `skills/<id>/SKILL.md` (Agent Skills format: `name`, `description`) + supporting files |
| Agent | A named role with a system prompt, preferred tier or model, allowed tools, and attached skills and rules (e.g. "Test writer", "Security reviewer") | `agents/<id>.md` with frontmatter `displayName`, `description`, `tier`, `model` (optional), `tools`, `skills`, `rules` |

**Where items live (scopes):**

- **Global:** `<data>/library/{rules,skills,agents}/`, available in every workspace.
- **Workspace:** `.orchestrator/library/{rules,skills,agents}/`, can be committed so the team shares it. A workspace item with the same `id` overrides the global one.
- **Profiles:** named sets of enabled items (e.g. "Frontend", "Rust backend"); each workspace picks one. The default profile enables everything.
- **Starter pack:** `catalog/library-starter/` ships a few editable examples (a "Test writer" agent, a "Conventional commits" rule, a "Write migration" skill).

**Sync to each CLI (the `library/` module):**

The core writes the resolved Library into each installed CLI's native locations inside the workspace. It does this before a run, and when you save an item while the Library screen is open. Sync is incremental: only files whose content hash changed are rewritten. Each CLI has a sync writer; indicative targets below (Claude must verify every path against the CLI's docs at build time, rule 7):

| CLI | Rules | Skills | Agents |
| --- | --- | --- | --- |
| Claude Code | managed block in `CLAUDE.md` | `.claude/skills/<id>/` | `.claude/agents/<id>.md` |
| Cursor Agent | `.cursor/rules/<id>.mdc` | native skills folder if supported, else referenced from rules | converted to a rule plus a task-file role header |
| Codex | managed block in `AGENTS.md` | native skills folder if supported | task-file role header |
| Copilot | `.github/copilot-instructions.md` managed block | native skills folder if supported | `.github/agents/` if supported, else task-file role header |
| OpenCode | managed block in `AGENTS.md` | native skills folder if supported | `.opencode/agent/<id>.md` |
| Other open-source agents | their native rules file if any, else `AGENTS.md` | native if supported | task-file role header |

- **Fallback that always works:** if a CLI has no native slot for a kind, the core puts the item's content into the step's task file (`tasks/<n>.md`) under "Role", "Rules" and "Skills available". Every agent therefore gets the same instructions even without native support.
- **Never clobber user files:** in shared files (`AGENTS.md`, `CLAUDE.md`, `copilot-instructions.md`) the core only edits between `<!-- orchestrator:begin -->` and `<!-- orchestrator:end -->` markers (marker text uses `shortName`). Whole generated files start with a "generated by <productName>, edit in the Library" header and are listed in `.orchestrator/managed.json`. If a user edits a generated file, the core sees the hash change and offers to import the edit into the Library instead of overwriting it.
- **Import:** on first open of a workspace, scan for existing `.cursor/rules`, `.claude/skills`, `.claude/agents`, `CLAUDE.md`, `AGENTS.md` and Copilot instructions and offer to import them into the Library.
- **Git hygiene:** generated CLI files are git-ignored by default (only the canonical `.orchestrator/library/` is suggested for commit); the user can change this per workspace.

**How the router uses Library agents:**

- A plan task can name a Library agent (`[agent: test-writer]`); the planning prompt lists available agents with their descriptions so the planner can pick.
- A Library agent's `tier` (premium / cheap / local / any) is a hint the router respects unless the mode, the governor, or a hard rule overrides it; the reason is shown in the UI.
- Each run stores a `library_snapshot_hash` so a run can be reproduced with the exact skills and rules it used.

**Library screen:** a tree of Rules, Skills and Agents with scope badges (Global / Workspace), enable toggles, inline rename, and a CodeMirror editor with a live preview of what each CLI will receive. It shows each CLI's sync status ("synced", "not supported" or "fallback"). You can duplicate items, move them between scopes, and delete them; delete lists the agents that reference the item first.

## Coding agents

The app orchestrates existing coding agents; it does not ship its own. There are two families, both curated in `catalog/agents.json`.

**Paid coding agents** (planner, reviewer, and high steps). They use the user's own subscription or key, and the app never stores their credentials:

| Agent | Milestone | Notes |
| --- | --- | --- |
| Cursor Agent (CLI) | M1 | Headless print mode with JSON output |
| Claude Code | M4 | Print mode with stream-JSON output |
| Codex CLI | M4 | `codex exec` with JSON output; also check its local-model (OSS/Ollama) mode, which would let it act as an open-source-style executor |
| GitHub Copilot CLI | M4 | Non-interactive mode |

**Open-source coding agents** (executors for low and trivial steps). They drive a downloaded local model through a local runtime, or a cheap cloud model:

| Agent | Milestone | Why |
| --- | --- | --- |
| OpenCode | M1 (default) | Headless `run` with JSON output; supports Ollama and OpenAI-compatible providers; has agents and rules |
| Aider | M4 | Mature, scriptable, strong with local models, git-native |
| Goose | M4 | Headless mode, many providers, extensible |
| Candidates to evaluate | M4+ | Qwen Code, Crush, Cline CLI, Kilo CLI. Add when they have a stable headless mode, JSON or parseable output, and reliable local-model tool calling |

Each catalog entry lists: id, default display name, family (paid / open_source), install command per OS, detect and version command, auth command, headless command template, supported providers and runtimes, Library sync targets, known-good version, and docs link. Before adding an agent, Claude verifies its headless mode and local-model support against the current docs and records a fixture (rule 7).

The default pairing shown in onboarding is **your paid agent + OpenCode on a local model** (or on a cheap cloud model if no local model fits).

## Agent adapter layer

Every agent CLI sits behind one Rust trait, so the router never knows which CLI it is driving. Adding an agent means adding one module and one Library sync writer, and nothing else.

```rust
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn family(&self) -> AgentFamily;          // Paid | OpenSource
    fn supported_providers(&self) -> &[ProviderType]; // which model backends it can drive
    async fn detect(&self) -> DetectResult;   // installed? version? path?
    async fn check_auth(&self) -> AuthStatus; // logged in / key present
    async fn install(&self, tx: ProgressTx) -> Result<()>;
    async fn configure_model(&self, ws: &Workspace, model: &ModelRef) -> Result<()>;
    async fn run(&self, req: StepRequest, tx: EventTx) -> Result<StepOutcome>;
    async fn cancel(&self, step_id: StepId) -> Result<()>;
}
```

`StepRequest` holds:
- the workspace path and the task file path;
- a `ModelRef` (provider + model name, or none to use the CLI's own default);
- the Library agent id, if any;
- a timeout and a process priority (from the governor).

`run` spawns the CLI headless in the workspace and parses its output stream into normalized events (`Stdout`, `ToolCall`, `FileEdit`, `Tokens{in,out}`, `Error`). It returns success, failure, or cancelled. Parsing is streaming and allocation-light: no buffering of whole outputs. API keys reach the CLI only through environment variables read from the keychain at spawn time.

**Fake adapter:** `fake` (test, M1) is a scripted binary in `/tests/fake-cli` that replays recorded event streams.

For each real adapter, Claude must confirm the exact command, flags and output schema from the installed CLI's `--help` before writing the parser, and save a sample output under `/tests/fixtures/<adapter>/`. Where an agent supports ACP (Agent Client Protocol), prefer one shared ACP client over a custom parser.

**Rules for all adapters:** run with the workspace as the working directory; never pass secrets on the command line; kill the whole process tree on cancel; enforce a per-step timeout (default 15 minutes); record the CLI version and model with every run.

## Local models, runtimes and cloud providers

The open-source agent needs a model. That model comes from a local runtime on the Mac, or from a cheap cloud provider, so the app works without a local model.

**Tiers:**

| Tier | Source | Needs |
| --- | --- | --- |
| Local | A model downloaded into a local runtime (Ollama by default) | Enough memory (hardware check + governor) |
| Cheap cloud | Any OpenAI-compatible or direct provider (OpenRouter, DeepSeek, Gemini, Anthropic, OpenAI, Groq, …); the user picks the model from the provider's list | An API key |
| Premium | The paid agent's own model | The user's existing subscription or key |

**Local runtimes (`catalog/runtimes.json`):**

- **Ollama** (default): the app talks to its HTTP API on `localhost:11434`.
- **Also supported as OpenAI-compatible endpoints:** LM Studio, a llama.cpp server, and an MLX server (Apple Silicon native, often faster and lighter on Macs). The app detects them if running; installing them is optional.
- The Agents & Models screen shows which runtime serves each model. The governor treats all runtimes the same, using process memory plus the runtime's own loaded-model report where available.

**Model storage directory:**

- **Reuse what's there:** on first launch, detect existing model stores (Ollama's default `~/.ollama/models`, or a custom path set through Ollama's models-directory env var; LM Studio's models folder). The app lists and uses models already downloaded, and never re-downloads a model the user already has.
- **Choose where models go:** Settings → Models → "Model folder" lets the user pick any directory, e.g. an external SSD, with free space shown.
  - When the app manages the Ollama service, it starts it with that directory as the models path.
  - When the user runs Ollama themselves, the app shows the exact setting to change instead of changing it silently.
- **Move models:** "Move models to…" copies with progress, verifies, then removes the old copy. It is cancellable and resumable.
- **Import:** add a GGUF file or folder from disk to the runtime without copying it where the runtime allows.
- Disk usage per model and total is always visible, with one-click remove.

**Recommended local models:** `catalog/models.json` holds a short, hardware-filtered suggestion list for coding with tool calling. It is not a showcase: onboarding suggests the single best fit for the machine, and the full list stays one click away. Entries record parameter count, quantization, download size, memory needed at the default context, context length, tool-calling support, and preferred runtime per platform.

**Cloud providers (`providers/` module):**
- **Provider types:** `openrouter`, `openai_compatible` (any base URL), and direct `anthropic`, `openai` and `gemini` entries, plus a `fake` provider for tests.
- **Per provider:** list models, test the key with a tiny request, and read prices where the API exposes them. The user can edit prices.
- **No curated cloud model list:** the app marks each model's tool-calling support and price so the user can choose.

**Executor pool:** Settings has an ordered pool of (open-source agent, model) pairs, e.g. `[OpenCode + local qwen-coder-14b, OpenCode + cloud deepseek-chat]`. The router uses the first entry the governor allows. It falls through to the next entry, and logs why, when:
- the local model isn't installed;
- the runtime is down;
- memory is over budget;
- the Mac is hot, or on battery with "prefer cloud" on.

A Mac with no local model simply has a pool with only cloud entries.

**Keys and privacy:**

- API keys live in the macOS Keychain; the UI shows only the last 4 characters. Keys are never written to files, logs or the database.
- Before the first run on a cloud model, show a one-time notice that code from the workspace will be sent to that provider. Workspaces marked "local only" have cloud entries removed from their pool.
- Per-provider monthly spend cap (estimated), with a pause when reached.

## Router and modes

The router assigns every step to a premium, cheap-cloud or local executor. It uses the active mode, a rule-based classifier, the Library agent's tier hint, the executor pool and the governor's current limits. The user can override any assignment before or during a run.

In the tables below, **"cheap"** means the first executor-pool entry the governor allows: a local model if one is set up and fits right now, otherwise a cheap cloud model.

**Modes:**

| Mode | Planning | Execution | Review | Escalation to paid |
| --- | --- | --- | --- | --- |
| Cost | Paid (short plan) | Cheap for every step (local preferred) | None | Only after 2 failed attempts and user approval |
| Balanced (default) | Paid | Cheap for low steps, paid for high steps | Paid, once at the end | Automatic after 2 failed attempts |
| Intelligent | Paid | Paid for everything except trivial steps | Paid, once at the end | Not needed; cheap only for trivial steps |

**Escalation ladder:** local → cheap cloud (if in the pool) → paid. A failure on a local model goes to cheap cloud before paid when both exist, so escalation stays as cheap as possible.

**Budget planning (ships in v1, off by default):** users with no paid agent, or who want the lowest cost, can set a stronger cheap-cloud model as planner and reviewer. With it on, "Paid" in the table above means that model for planning and review. Execution still follows the mode. Escalation goes to that model, since no paid agent exists. The UI labels runs "Budget planning" and tracks their pass rate separately so the user can compare. Onboarding offers it automatically when no paid agent is detected.

**Step classifier (v1, rules only):**

- High: architecture or design, debugging an unknown failure, changes across more than 3 files, security or auth code, concurrency, data migrations, anything the plan marks `[high]`.
- Low: boilerplate, writing tests for existing code, renames, formatting, docs and comments, config edits, commit messages, single-file changes the plan marks `[low]`.
- Trivial (Intelligent mode's cheap work): formatting, commit messages, changelog entries.
- Unclear: treat as high in Intelligent, low in Cost, high in Balanced.

The planning prompt asks the planner to tag each task `[high]` or `[low]` with a one-line reason and, optionally, a Library agent. The rule set overrides the tag only when a hard rule matches, and every decision is stored with its reason so the UI can show why.

**User control:**

- Switch mode per run, and change it mid-run for the remaining steps.
- Reassign any pending step to a different agent, model or Library agent with one click.
- "Approve before paid" toggle: pause before every paid call. Separate "approve before cloud" toggle for cheap cloud calls.
- Budget cap per run: estimated paid tokens, cloud spend, or steps; pause when reached.
- Rename modes (display name only) and duplicate a mode to tweak its table.

A learned router (from past run outcomes) is out of scope until M5.

## Context handoff and verification

Agents never share sessions, so all context passes through files in the workspace that every CLI can read. A step is not done until the verification gate passes.

**Handoff files (in `.orchestrator/`, git-ignored by default except `library/` and `workflows/`):**

| File | Written by | Contents |
| --- | --- | --- |
| `plan.md` | Planner | Goal, numbered tasks with `[high]`/`[low]` tags, optional `[agent: <id>]`, files likely touched, acceptance check per task |
| `tasks/<n>.md` | Core | One task: goal, relevant excerpt of the plan, files to touch, constraints, the check that must pass, notes from earlier steps, and the Library role, rules and skills for this step (fallback injection) |
| `progress.md` | Core | Done steps with one-line summary, agent + model used, and changed files; appended after each step |
| `failures/<n>-<attempt>.md` | Core | Failed check output and diff summary, fed to the retry or escalation |
| `managed.json` | Core | List and hashes of generated CLI files from the Library sync |

Task files are kept short (target under 2k tokens) so small local models don't lose the prompt: only the relevant plan excerpt and the last few progress lines are included.

Every agent is started with the instruction to read `AGENTS.md`, `plan.md`, `progress.md` and its task file first. The user's own rules stay in their files; Library content goes only inside managed blocks.

**Verification gate:**

1. After each step, run the workspace's check commands from settings (auto-detected from `package.json`, `Cargo.toml`, `pyproject.toml`, `Makefile`; user can edit). Checks run at the same lowered priority as agents.
2. Also check the step's own acceptance line from `plan.md` when it names a command.
3. Pass: commit the step on a working branch (`orchestrator/<run-id>`), or snapshot it in a non-git workspace, and continue.
4. Fail: write the failure file and retry on the same executor once, then escalate per the mode and ladder.
5. The user can always accept a failed step manually or roll it back to the previous step's commit or snapshot.

Working on a branch with one commit per step makes every step reversible and gives the user a clean diff to review.

## Installers

Users install coding agents, a local runtime and models from inside the app, the app only recommends models that fit their hardware, and users who can't or don't want to run local models connect a cheap cloud provider instead.

**Agent installer:**

- Driven by `catalog/agents.json` (shipped with the app, signed, updatable).
- Install runs the command in a visible log panel; the user sees every command before it runs and must confirm it.
- After install: detect version, then guide auth (open the CLI's own login flow; the app never stores the user's paid-agent credentials itself).

**Runtime installer:**

- Ollama is installed the same way (Homebrew or official installer, per catalog) and started only while the app needs it, unless the user sets it to always run. LM Studio and MLX are detected, not required.

**Model installer (local):**

- Hardware check at onboarding: chip, total unified memory, free disk on the chosen model folder.
- Show each suggested model as Fits / Tight / Won't fit for this machine; hide Won't fit by default.
- Download into the chosen model folder with the runtime's pull API, streaming progress to the UI; allow pause, resume and cancel. Downloads run at low network and disk priority.
- After download, set the context window (default 16k on ≤ 16 GB Macs, 32k above, capped so the governor's memory budget holds) so agents don't lose their prompt without overloading memory.
- One click to connect a model to an open-source agent: write the agent's config (for OpenCode, its provider and model entries) and run a 10-second smoke test.

**Cloud provider setup (no local model needed):**

- `catalog/providers.json` lists providers with signup link, key format and base URL.
- "Connect a provider": paste a key (stored in the Keychain), and the app tests it and lists models, marking price and tool-calling support. The user picks one. One click connects it to the open-source agent: the app writes the provider config with an env var reference, never the key itself, then runs the same smoke test.
- If the hardware check shows no local model fits, onboarding recommends this path first.

**Uninstall:** remove models through the runtime and agents through their package manager, with a size-freed readout; disconnecting a provider deletes its Keychain entry.

## Telemetry and resource dashboard

The user can see, live and afterwards, which agent and model did what, what it cost, and how much memory, CPU and energy each model and tool is using.

**Live run view:**

- Step list with status, assigned agent, model, tier badge (Local / Cheap cloud / Premium), Library agent, and the router's reason.
- Streaming log per step (virtualised): agent output, tool calls, files edited, check results, governor events.
- Running totals: premium steps, cheap-cloud steps, local steps, tokens in and out, estimated cost per tier, time.

**Resource view:**

- Sampling every 2 s while a run is active or this view is open; otherwise off (the governor's pressure listener is event-based and costs nothing).
- Per agent process tree: RAM (RSS) and CPU, summed across child processes.
- Per local model: memory from the runtime's loaded-model report (for Ollama, `/api/ps`), plus the time left before it unloads.
- System totals: memory used and free, macOS memory-pressure level, thermal state, power source.
- Stacked area chart over time and a current-usage bar per model and tool; "Why is my Mac slow?" button (see Resource governor).
- The app's own footprint is listed too, so the user can see it stays small.

**Cost estimate:**

- Token counts come from each CLI's output when it reports them; otherwise estimate from prompt and output length (about 4 characters per token) and mark it as estimated.
- The user sets a price per million tokens per paid agent, or an effective cost per request for subscription tools like Cursor. Cloud prices come from the provider or are set by the user. Local models cost zero (optionally, a user-set electricity cost). The app never claims exact billing.
- Per run, show "saved vs all-paid": what the cheap steps would have cost on the paid agent.

**History:** a runs table with workspace, mode, steps, share per tier, estimated cost, peak memory, duration and result; filter by workspace and date. Export a run as JSON (including its Library snapshot hash and model list).

Nothing leaves the machine except the model calls the user configured. If crash reporting is added later, it must be opt-in and off by default.

## UI, onboarding and workflow builder

Onboarding gets a new user from launch to a first routed task in under 2 minutes, in five short screens with no forms longer than one field.

**Onboarding flow:**

1. **Scan:** a quick sweep detects installed coding agents, local runtimes, already-downloaded models (in Ollama's folder or others), chip and memory. It also finds existing skills, agents and rules in common locations. Results show as cards lighting up. The scan must take under 2 s.
2. **Pick your style:** three large cards for Cost, Balanced, Intelligent, each with a one-line promise and an example cost bar. Balanced is preselected. If no paid agent is found, offer budget planning here.
3. **Choose your executor:** two cards.
   - "Run locally": shows OpenCode + the best-fit model. If a model is already downloaded, "use existing" is the default. Otherwise it shows e.g. "Download Qwen Coder 7B (4.7 GB) to ~/.ollama/models", with a Change folder link.
   - "Use a cheap cloud model": paste one key.
   
   If no local model fits, cloud is preselected. Both show a progress bar and a smoke test.
4. **Pick a workspace:** open a folder, or use the bundled sample project. Offer to import any skills, agents and rules found there.
5. **First run:** a task ("add a unit test to this project") runs live so the user watches the planner hand off to the executor, with the status bar showing memory in use.

Keep it fun but fast: short copy, subtle motion (reduced when macOS "Reduce motion" is on), a small celebration on the first passing run. Every step has Skip.

**Main screens:**

| Screen | Purpose |
| --- | --- |
| Home | Workspace switcher (open folder, recent, pinned), type a goal, choose a mode, pre-flight badge, Run |
| Run | Live step list, logs, reassign and approve controls, cost and memory totals |
| Library | Skills, agents and rules: create, edit, rename, enable, scope, profiles, per-CLI sync status |
| Agents & Models | Paid and open-source agents, local runtimes, models on disk with the model folder, cloud providers; install, connect, move, remove; executor pool order |
| Telemetry | Resource charts, "Why is my Mac slow?", run history |
| Workflows (M5) | Visual builder |
| Settings | Check commands, budgets, prices, approval rules, memory budget, battery/thermal behaviour, model folder, privacy (local-only workspaces), theme |

A status bar with live resources sits at the bottom of every screen, and an optional menu bar item offers Stop everything.

**Renaming everywhere:** any item with a display name (workspace, skill, agent, rule, profile, mode, workflow, model, provider) can be renamed inline; the change is saved immediately and reflected across the app.

**Visual workflow builder (M5):**

- React Flow canvas with node types: Plan, Execute, Verify, Review, Condition, Human approval.
- Each node picks an agent (or "router decides"), a model or tier, and optionally a Library agent; edges carry pass and fail branches.
- The three modes ship as built-in templates the user can copy, rename and edit.
- Workflows save as JSON in `.orchestrator/workflows/` so they can be versioned with the workspace.

## Milestones and acceptance criteria

Five milestones after the skeleton; M1–M2 are the MVP (about 6–8 weeks solo with Claude), M3–M4 make it v1, M5 is v2. Claude ticks each box only when it is verified.

**M0 — Skeleton and performance harness**

- [ ] Tauri 2 + React app launches on macOS arm64; CI builds on macOS, Windows, Linux
- [ ] `scripts/perf` and a CI perf job measuring cold start, idle memory, idle CPU and bundle size against the budgets
- [ ] `brand.json` + `apply-brand` script; changing `productName` and rebuilding updates the window title, bundle and UI copy with no code edits
- [ ] SQLite schema from the data model, with migrations, WAL and batched writes
- [ ] `AGENTS.md`, `docs/DECISIONS.md`, `docs/perf.md` and lint/test commands in place

**M1 — One paid, one open-source agent, end to end, in any folder**

- [ ] `AgentAdapter` trait, `fake` adapter, `fake` provider, and integration tests driving a full run with the fake CLI
- [ ] `cursor` and `opencode` adapters with recorded fixtures and streaming parsers
- [ ] Workspace picker: open folder, drag-and-drop, recent list; git and non-git workspaces both run (non-git uses snapshots)
- [ ] OpenCode runs with either an Ollama model (existing models in the Ollama folder are detected and used) or an OpenAI-compatible cloud model (key from Keychain); tested with the fake provider
- [ ] Planner writes `plan.md`; core parses it into steps; handoff files written per step
- [ ] Balanced mode only, rule classifier, executor pool fallback (local → cloud), manual reassign per step
- [ ] Governor basics: pre-flight memory estimate, memory budget, one local model at a time, idle unload, lowered process priority
- [ ] Home and Run screens show live steps and virtualised logs; status bar shows live resources; cancel and Stop everything kill every process tree
- [ ] Perf budgets still pass with a run streaming 10k log lines

**M2 — Modes, verification, budget planning and the Library core**

- [ ] Cost, Balanced and Intelligent modes with the escalation rules and ladder in the Router section
- [ ] Budget planning (cheap-cloud planner and reviewer), off by default, offered when no paid agent is found
- [ ] Verification gate with auto-detected check commands; one commit (or snapshot) per passed step; rollback per step
- [ ] Approve-before-paid and approve-before-cloud toggles; per-run budget cap
- [ ] Library store (global + workspace scopes, ids vs display names), task-file fallback injection, incremental sync writers for Cursor and OpenCode with managed blocks and no clobbering
- [ ] Library screen: create, edit, inline rename, enable, delete; import from existing `.cursor/rules` and `AGENTS.md`
- [ ] A demo run on a sample repo completes in each mode: once with a local executor, once cloud-only, once with budget planning

**M3 — Installers, governor, onboarding, telemetry**

- [ ] Hardware check and Fits / Tight / Won't fit suggestions; model folder picker; move models between folders; import GGUF
- [ ] One-click install for Ollama, OpenCode and the suggested models, with progress, pause, cancel and smoke test
- [ ] Cloud provider connect flow (OpenRouter + one OpenAI-compatible provider) with key test, model picker and smoke test
- [ ] Full governor: memory-pressure, thermal and battery responses with plain-language events and notifications; menu bar item with Stop everything
- [ ] Five-screen onboarding finishes in under 2 minutes on a Mac that already has Cursor, on both the local and the cloud path
- [ ] Resource view with per-model and per-tool memory, "Why is my Mac slow?", run history, cost per tier with "saved vs all-paid"

**M4 — More agents and full Library sync**

- [ ] Paid adapters: `claude`, `codex`, `copilot`; open-source adapters: `aider`, `goose`; each with fixtures and tests
- [ ] LM Studio and MLX runtimes supported as OpenAI-compatible local endpoints
- [ ] Library sync writers for Claude Code, Codex, Copilot, Aider and Goose; import from `.claude/skills`, `.claude/agents`, `CLAUDE.md`, Copilot instructions
- [ ] Library profiles per workspace; plan tasks can name a Library agent and the router honours its tier hint
- [ ] Any paid agent can be planner or reviewer; any open-source agent can be the executor with any supported runtime or provider

**M5 — Workflow builder and smarter routing**

- [ ] React Flow builder with the node types in the UI section; modes shipped as editable, renameable templates
- [ ] Router learns from past runs in the same workspace (which step types passed on which tier and model) and suggests reassignments

## Risks and open questions

There are two biggest risks:
- **Cheap executors fail too often,** so the cheap path costs more than going paid directly. The verification gate, the escalation ladder and the "saved vs all-paid" readout exist to catch that early.
- **Local models make the Mac unusable.** The governor and its always-visible status exist to prevent that.

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Local models overload memory or heat the Mac | Mac becomes sluggish, user loses trust | Memory budget, one model at a time, idle unload, pressure/thermal/battery responses, lowered priority, visible status and Stop everything |
| The app itself grows heavy | Defeats the product's promise | Hard budgets, CI perf gate, lazy-loaded screens, no polling when idle |
| Local or cheap model quality too low | Retry loops, wasted time, escalations eat savings | Small, specific tasks in short task files; retry cap of 2; ladder through cheap cloud before paid; track pass rate per model and warn |
| Budget planning produces weak plans | More failed steps | Off by default; pass rate tracked separately; easy switch back to a paid planner |
| Duplicate model downloads fill the disk | Wasted space | Reuse existing Ollama/LM Studio stores; one model folder setting; disk usage always visible |
| CLI flags, output formats or native config folders change | Adapters or Library sync break after an update | Record CLI version per run; fixture tests; prefer ACP where supported; task-file fallback for Library; pin known-good versions in the catalog |
| Library sync overwrites user files | Lost user work, loss of trust | Managed blocks only; hash check on generated files; import instead of overwrite; generated files listed in `managed.json` |
| Code sent to cloud providers | Privacy or compliance concerns | One-time notice; "local only" workspaces; per-provider enable; keys in Keychain |
| Cloud API spend creeps up | Unexpected bills | Per-provider and per-run caps; approve-before-cloud toggle; cost per tier shown live |
| Agents write outside the chosen folder | Damage to unrelated files | Workspace as cwd; CLI sandbox/allow-list flags where available; out-of-scope edit warnings; confirm unsafe roots |
| Cursor limits or changes headless CLI use | Primary paid path breaks | Claude Code, Codex and Copilot adapters as alternatives (M4) |
| Subscription usage is not exposed | Cost figures are guesses | Label all costs as estimates; let users set their own effective price |
| Running user-approved install commands | Security exposure | Show every command before it runs; signed catalog updates; no silent installs |

**Decided:**

- [x] Budget planning (cheap-cloud planner and reviewer) ships in v1, off by default (rev 3).
- [x] The curated catalog is of coding agents (paid and open-source), not cloud models; cloud models are picked by the user from each provider's list (rev 3).
- [x] Models download into the Ollama directory by default, or a folder the user chooses; existing downloads are reused (rev 3).

**Open questions:**

- [ ] Product name (working title: Hybrid Agent Orchestrator; changeable any time via `brand.json`)
- [ ] Free and open source, or paid app?
- [ ] Should `.orchestrator/` be committed to the repo or git-ignored by default? (Current default: ignore all except `library/` and `workflows/`.)
- [ ] Which open-source agents join OpenCode in M4? (Proposed: Aider and Goose; evaluate Qwen Code, Crush, Cline CLI, Kilo CLI.)
- [ ] Should the MLX runtime be promoted to the default on Apple Silicon if it proves faster and lighter than Ollama in our benchmarks?
- [ ] Should generated CLI files from the Library be committed by default for teams?
