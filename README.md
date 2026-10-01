# Hybrid Agent Orchestrator

A light, native-feeling Mac app that sends high-thinking coding work to your **paid coding agent** (Cursor Agent, Claude Code, Codex, Copilot) and routine work to an **open-source agent** (OpenCode, Aider, Goose) running on a **local model** (Ollama, LM Studio, llama.cpp, MLX) or a **cheap cloud model**. You pick a folder and a mode, watch every handoff, and can override any routing decision.

> The product name lives in `brand.json`; change it there and rebuild.

## Highlights

- **Three modes:** Cost, Balanced, Intelligent (rename or duplicate them, or design your own in the workflow builder).
- **Efficient and transparent:** ~0.4 s cold start, ~86 MB idle, 0% idle CPU, 7.7 MB app (`docs/perf.md`). A resource governor enforces a memory budget, keeps one local model loaded, unloads idle models, reacts to memory pressure, heat and battery, and always says why. A status bar and menu bar item show what's running, with **Stop everything**.
- **Verification gate:** your tests/lint run after every step; one commit per step on a working branch (or snapshots without git); retries and an escalation ladder local → cheap cloud → paid; roll back any step.
- **Shared Library:** write rules, skills and agents once; they're synced into every installed CLI's native format, without ever clobbering your own files.
- **Installers:** one-click (confirmed) installs for agents and Ollama, model suggestions rated Fits/Tight/Won't fit for your Mac, reuse of models you already downloaded, a model folder you choose, cloud keys in the Keychain.

## Develop

```sh
npm ci
npm run app:dev        # run the app (debug builds include scripted demo agents)
npm test               # UI unit tests
npm run test:rust      # core unit + end-to-end runs with the fake agent CLI
npm run test:e2e       # UI in Chromium against a mocked core
npm run app:build      # release .app
npm run perf           # performance budgets
```

See `AGENTS.md` for structure and conventions, `docs/SPEC.md` for the spec, `docs/DECISIONS.md` for decisions and verified CLI interfaces.
