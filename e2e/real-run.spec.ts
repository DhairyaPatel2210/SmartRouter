import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";

const mock = readFileSync(new URL("./tauri-mock.js", import.meta.url), "utf8");

// Replays a real failed run (planner error, nothing planned) captured from a user's database.
const run = {
  id: "5fa59260", workspace_id: "w1", goal: "what is this repo about?", mode: "balanced", status: "failed",
  started_at: 1790818441063, ended_at: 1790818442717, est_cost_usd: 0.000882, peak_mem_mb: 0, library_snapshot_hash: "x",
  budget_planning: false, branch: "orchestrator/5fa59260", base_ref: "v1.3",
  summary: { base_commit: "78e3ab4", error: "The planner didn't produce a plan.", stashed: "Orchestrator: saved before run 5fa59260", steps_total: 0, tokens_estimated: true },
};
const plan = {
  id: "4c19", run_id: "5fa59260", idx: 0, title: "Plan the work", class: "high", kind: "plan", agent_id: "claude", model_id: null,
  tier: "premium", library_agent_id: null, status: "failed", attempts: 1, escalated: false, route_reason: "planning by the paid agent",
  tokens_in: 294, tokens_out: 0, tokens_estimated: true, cost_usd: 0.000882, paid_equiv_usd: 0.000882, commit_ref: null,
  started_at: 1790818441345, ended_at: null,
  detail: { agent_error: "Error: Input must be provided either through stdin or as a prompt argument when using --print", edits: [], executor: "Claude Code", model: null },
};

test("start a run that fails at planning: run view stays rendered", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await page.addInitScript(`window.__mockOverrides = { start_run: "5fa59260", get_run: ${JSON.stringify({ run: { ...run, status: "pending", summary: null }, steps: [], active: true, paused: false, workspace: null })} };`);
  await page.addInitScript(mock);
  await page.goto("/");
  await page.getByPlaceholder(/What should the agents do/).fill("what is this repo about?");
  await page.getByRole("button", { name: /^Run$/ }).click();
  // Core events as the real run produced them.
  await page.evaluate(([r, p]) => {
    const emit = (window as unknown as { __emit: (e: string, x: unknown) => void }).__emit;
    emit("orch://events", [{ type: "run", run: { ...(r as object), status: "pending", summary: null } }]);
    emit("orch://events", [{ type: "run", run: { ...(r as object), status: "planning", summary: {} } }, { type: "step", step: { ...(p as object), status: "pending", detail: null } }]);
    emit("orch://events", [{ type: "step", step: p }, { type: "notice", level: "error", text: "Run stopped: planner failed", run_id: "5fa59260" }, { type: "run", run: r }]);
  }, [run, plan]);
  await expect(page.getByText("what is this repo about?").first()).toBeVisible();
  await expect(page.getByText(/Input must be provided/).first()).toBeVisible();
  expect(errors).toEqual([]);
});
