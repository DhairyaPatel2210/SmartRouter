# AGENTS.md

Rules and commands for any coding agent (and human) working on this repo.
Read `docs/SPEC.md` first; log non-obvious choices in `docs/DECISIONS.md`.

## Stack

- **Shell:** Tauri 2 (system WebKit). One Rust core owns processes, routing, the Library, the governor and metrics. The React UI only renders and sends commands.
- **Core:** Rust 1.94 (pinned in `rust-toolchain.toml`), tokio, rusqlite (WAL, batched writer), reqwest (rustls), sysinfo (targeted refresh), keyring (macOS Keychain), objc2 + raw FFI for macOS signals.
- **UI:** React 18 + TypeScript + Vite 8, Tailwind 4, zustand, TanStack Virtual, CodeMirror 6 (lazy), React Flow (lazy). No chart library (custom SVG).
- **Tests:** `cargo test` (unit + fake-CLI integration), Vitest, Playwright (UI against a mocked core), `scripts/perf.mjs`.

## Commands

| Task | Command |
| --- | --- |
| Install | `npm ci` |
| Run the app (dev) | `npm run app:dev` (demo agents on in debug builds) |
| Build the app | `npm run app:build` (or `npx tauri build --bundles app`) |
| Apply brand.json | `npm run brand` (runs automatically before dev/build) |
| UI unit tests | `npm test` |
| UI end-to-end | `npm run test:e2e` |
| Core tests | `npm run test:rust` |
| Lint | `npm run lint` and `npm run lint:rust` |
| Perf gate | `npm run perf` (needs a release build) |
| Fake agent / provider | `tests/fake-cli/fake-agent`, `tests/fake-provider/fake-provider 18777` |

Env vars: `ORCH_DATA_DIR` (data dir override), `ORCH_DEMO_AGENTS=1` (register the scripted demo agents in release), `ORCH_PERF=1` (print `ORCH_READY <ms>` when interactive).

## Layout

```
brand.json                 product naming (single source) → scripts/apply-brand.mjs
catalog/                   agents, runtimes, models, providers, library-starter, sample-project
src-tauri/src/
  adapters/                AgentAdapter trait, shared CLI runner, one module per CLI, fake CLI
  providers/               Ollama, OpenAI-compatible/cloud, Keychain, fake OpenAI server
  governor/                memory budget, pre-flight, local gating, app-managed Ollama
  router/                  classifier, modes, tier hints, pool fallthrough, escalation ladder
  engine/                  Core, run lifecycle (run.rs), routing env + pre-flight (env.rs)
  workspace/               inspect, git, non-git snapshots, scope guard
  library/                 store, resolver, per-CLI sync writers, import
  handoff/                 plan parser, task files, prompts
  verify/                  check detection and the verification gate
  installer/               catalog, hardware check, suggestions, model moves, GGUF
  telemetry/               batched UI bus, step logs, sampler, cost
  macos/                   pressure dispatch source, thermal, power, libproc
  db/                      schema/migrations, batched writer, queries
  commands.rs              every Tauri command the UI can call
src/                       React app: home/ runs/ library/ agents/ telemetry/ workflow/ settings/ onboarding/ statusbar/
tests/                     fake-cli, fake-provider, fixtures/<adapter>/
e2e/                       Playwright specs + Tauri IPC mock
```

## Conventions

- Never hard-code the product name: Rust uses `crate::brand::*`, TS uses `src/brand.ts` (both generated).
- Library items: stable `id` (slug) for references; `displayName` is what users see and rename.
- Agent output is parsed line by line; never buffer whole outputs. Raw stdout goes to log files; only normalized events go to SQLite.
- No new always-on timer, thread or >1 MB dependency without a DECISIONS entry and a passing perf job. Sampling runs only while a run is active or the Resource view is open.
- Never call a real paid agent or cloud model in automated tests: use `fake-paid`/`fake-oss` and the fake provider.
- Before using any external CLI flag, output format or config path, verify it against the CLI's `--help`/docs and record it in DECISIONS.
- Secrets: Keychain only; reach CLIs through env vars at spawn time; never in files, logs or the DB.
- Commits: small, one concern each, conventional-commit style.

## Adding an agent

1. `src-tauri/src/adapters/<id>.rs`: implement `CliSpec` (`bin`, `build`, `parse`) and `AgentAdapter` (use `impl_cli_run!()`).
2. Register it in `adapters::registry`, add it to `catalog/agents.json`, add a sync writer case in `library/sync.rs`.
3. Record a fixture under `tests/fixtures/<id>/` and a parser test.
