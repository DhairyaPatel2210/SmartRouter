# Decisions

Non-obvious choices, newest last. Format: date · choice · reason.

- 2026-09-30 · The user pre-approved building all milestones without stopping at each plan for confirmation ("go ahead and implement"). Plans are still written to `docs/plans/`. · Explicit user instruction.
- 2026-09-30 · Commits are authored by the repo owner only, with no co-author trailers. · Explicit user instruction.
- 2026-09-30 · `rust-toolchain.toml` pins Rust 1.94.0. · Current `tauri`/`tray-icon` need rustc ≥ 1.90; pinning per project avoids touching the user's global toolchain.
- 2026-09-30 · Toolchain versions: Node 22, Vite 8, TypeScript 6, React 18, Tailwind 4, ESLint 10, Vitest 5. · Latest mutually compatible majors at build time.
