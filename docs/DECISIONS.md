# Decisions

Non-obvious choices, oldest first. Format: date · choice · reason.

## Process

- 2026-09-30 · The user pre-approved building all milestones without stopping to confirm each plan. Plans are still written to `docs/plans/`. · Explicit user instruction.
- 2026-09-30 · Commits are authored by the repo owner only, with no co-author trailers. · Explicit user instruction.

## Toolchain & dependencies

- 2026-09-30 · `rust-toolchain.toml` pins Rust 1.94.0. · Current `tauri`/`tray-icon` need rustc ≥ 1.90; pinning per project avoids touching the user's global toolchain.
- 2026-09-30 · Node 22, Vite 8, TypeScript 6, React 18, Tailwind 4, ESLint 10, Vitest 5. · Latest mutually compatible majors at build time.
- 2026-09-30 · `rusqlite` (bundled) instead of `sqlx`. · Synchronous, no compile-time DB, smaller; all hot-path writes go through one batching thread anyway.
- 2026-09-30 · No Recharts: the memory chart is ~40 lines of SVG. · Efficiency rule 10 (Recharts + d3 would add ~100 KB gz for one chart).
- 2026-09-30 · Library editor uses CodeMirror core with a small custom markdown/frontmatter stream mode instead of `@codemirror/lang-markdown`. · lang-markdown pulls in HTML/CSS/JS parsers (editor chunk 182 → 98 KB gz). Loaded only with the Library screen.
- 2026-09-30 · `tauri-plugin-single-instance` added. · Lets `orch <path>` open a folder in the running app.
- 2026-09-30 · Playwright tests run the UI against a mocked Tauri IPC (`e2e/tauri-mock.js`). · Tauri's WebKit webview can't be driven by Playwright on macOS; the native core is covered by `cargo test` integration runs.

## Core design

- 2026-09-30 · The fake agent CLI and fake OpenAI-compatible provider are modes of the app binary (`--fake-agent`, `--fake-provider`); `tests/fake-cli` and `tests/fake-provider` are thin wrappers. · Zero extra runtime deps, works in CI and in demo mode; integration tests use `CARGO_BIN_EXE_orchestrator`.
- 2026-09-30 · Fake-agent scenarios can be set per workspace via `.orchestrator/fake-scenario`. · Tests run in parallel in one process; a process-wide env var would race.
- 2026-09-30 · Adapters spawn CLIs in their own process group with `nice 10` (pre_exec `setpriority`) and kill with `killpg` (TERM, then KILL after 2 s). · "Keep the Mac responsive" + "kill the whole process tree on cancel".
- 2026-09-30 · GUI-launched apps get a minimal PATH, so PATH is resolved once from the user's login shell (`$SHELL -ilc`) off the UI thread, plus common install dirs. · Otherwise Finder-launched builds can't find `claude`, `opencode`, `ollama`, etc.
- 2026-09-30 · Changed files per step come from git (`diff --name-only` + untracked) or snapshot manifests, not only from agent events. · Reliable for every CLI, including text-only ones.
- 2026-09-30 · Non-git snapshots are content-addressed (sha256 objects + per-step manifests) under `<data>/snapshots/<run-id>/`; heavy dirs are skipped; >1 GB aborts with "initialise git instead". · Rollback without copying the workspace per step.
- 2026-09-30 · The handoff dir gets its own `.gitignore` (`*` except `library/` and `workflows/`); generated CLI files are listed in `.git/info/exclude` inside a managed block. · Spec's git-hygiene defaults without editing the user's `.gitignore`.
- 2026-09-30 · Each run starts with a "chore: sync library" commit on its branch, then one commit per step. · Keeps Library managed-block edits out of step diffs.
- 2026-09-30 · Commits use the user's git identity; only if none is configured does the app pass a fallback identity via `-c`. · Never fail a run on a fresh machine; never impersonate otherwise.
- 2026-09-30 · Failure policy: retry on the same executor once (2 attempts), then one rung up the ladder; when the ladder is exhausted the failed changes stay in the tree and the user picks Retry / Keep changes / Skip / Stop. · Matches the spec and makes "accept a failed step manually" possible.
- 2026-09-30 · With no paid agent and budget planning off, the first usable cheap executor plans (and the UI recommends budget planning). · The app must still work with only cheap executors.
- 2026-09-30 · Memory-pressure "critical" cancels the running local step, unloads app-loaded models and asks: continue on cloud / retry / skip / stop. Partial changes are rolled back. · Spec: pause the local step, unload, offer cloud or wait.
- 2026-09-30 · Scope guard: paths an agent reports are checked against the workspace (lexical + canonical); in Cost mode a violation interrupts the step. Claude Code is confined by its own cwd rules; OpenCode gets `external_directory: deny`. · Spec scope guard + per-CLI sandbox flags.
- 2026-09-30 · Workflows (M5) compile into a routing mode ("Use as mode") rather than a separate graph interpreter. · Keeps one well-tested engine; the graph controls per-class tiers, review and escalation policy.
- 2026-09-30 · Router learning (M5) suggests reassignments from first-try pass rates (Telemetry → Routing insights) but never auto-applies them. · Spec: "suggests reassignments".

## Resource governor

- 2026-09-30 · Memory estimate for a GGUF model = `download × 1.1 + ctx/1024 × 0.05 × max(0.5, download/4.5) + 0.3` GB, always shown with "~"; catalog models use their measured `mem_gb` at 16k. · Good enough to rate Fits/Tight/Won't fit; Ollama's `/api/ps` gives the real number once loaded.
- 2026-09-30 · Fit: over budget → Won't fit; free after load < 0 → Won't fit; < 2 GB or > 85% of budget → Tight. · Matches the spec's example (1.5 GB after load = Tight).
- 2026-09-30 · Suggestions are rated against the budget and 75% of total memory, not momentary free memory. · A suggestion shouldn't flip because a browser tab is open; pre-flight uses live free memory.
- 2026-09-30 · Pressure events come from a libdispatch memory-pressure source registered via `dispatch_source_set_event_handler_f` (no blocks, no polling). Thermal/Low Power via `NSProcessInfo` (objc2-foundation), power source via IOKit `IOPSGetProvidingPowerSourceType`, process trees via `proc_listchildpids`. · Event-driven, tiny FFI surface.
- 2026-09-30 · Perf "idle memory" uses physical footprint (`vmmap --summary`) of the core plus the WebKit processes the app spawned. · RSS counts shared system frameworks in every process and overstates by ~2×; Activity Monitor reports footprint.

## Verified external interfaces (rule 7)

- 2026-09-30 · **Claude Code 2.1.278** (installed): `-p`, `--output-format stream-json`, `--verbose`, `--permission-mode acceptEdits`, `--allowedTools`, `--model`, `--agent`, `claude auth status --json` verified with `--help`. Fixture is synthetic to avoid spending the user's subscription.
- 2026-09-30 · **OpenCode 1.18.34** (npm `opencode-ai`, run from a scratch dir): `run --format json -m provider/model --dir --agent` verified with `--help`; `OPENCODE_CONFIG_CONTENT`, `OPENCODE_DISABLE_AUTOUPDATE`, `{env:VAR}` substitution, `.opencode/skills/<name>/SKILL.md`, `.opencode/agents/<name>.md` confirmed from the binary; JSON events (`step_start`, `tool_use`, `text`, `step_finish`) **recorded** against the fake provider (`tests/fixtures/opencode/run.jsonl`).
- 2026-09-30 · **Ollama 0.24.0** (installed): `OLLAMA_MODELS`, `OLLAMA_MAX_LOADED_MODELS`, `OLLAMA_KEEP_ALIVE`, `OLLAMA_CONTEXT_LENGTH`, `OLLAMA_NUM_PARALLEL` verified with `ollama serve --help`; API: `/api/version`, `/api/tags`, `/api/ps`, `/api/pull`, `/api/generate` (`keep_alive: 0` unload), `/api/show`, `/api/delete`, OpenAI-compatible `/v1`.
- 2026-09-30 · **Not installed on the build machine; implemented from docs, synthetic fixtures:** Cursor Agent (`cursor-agent -p --output-format stream-json --force`, `status`, `login`), Codex CLI (`codex exec --json --full-auto --skip-git-repo-check --cd`, `--oss -m`, `login status`), Copilot CLI (`copilot -p --allow-all-tools --no-color --model --agent`), Aider (`--message --yes-always --no-auto-commits --no-pretty --no-stream --read`), Goose (`goose run --no-session -t`, `GOOSE_PROVIDER/GOOSE_MODEL`). Re-verify and record real fixtures when each is installed.
- 2026-09-30 · Library native locations used: Claude `.claude/skills/<id>/SKILL.md`, `.claude/agents/<id>.md`, `CLAUDE.md` block; Cursor `.cursor/rules/<id>.mdc`; Copilot `.github/copilot-instructions.md` block, `.github/agents/<id>.agent.md`; OpenCode/Codex/Aider `AGENTS.md` block; Goose `.goosehints` block. Skills for Cursor/Codex/Copilot/Aider/Goose use the task-file fallback (copies in `.orchestrator/skills/`).
- 2026-10-01 · Homebrew install names verified with `brew info`: `opencode` (homebrew-core, replaces the unverified `sst/tap/opencode`), `codex` (cask), `aider`, `block-goose-cli`, `ollama`, `llama.cpp`. · Rule 7: verify before use.
