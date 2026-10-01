import { useApp } from "./store";
import { logStore } from "./logs";
import type { RunRow, StepRow } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const run = (status: RunRow["status"]): RunRow => ({
  id: "r1", workspace_id: "w", goal: "g", mode: "balanced", status, started_at: 1, ended_at: null, est_cost_usd: 0,
  peak_mem_mb: 0, library_snapshot_hash: null, budget_planning: false, branch: null, base_ref: null, summary: {},
});

const step = (id: string, status: StepRow["status"]): StepRow => ({
  id, run_id: "r1", idx: 1, title: "t", class: "low", kind: "execute", agent_id: null, model_id: null, tier: "local",
  library_agent_id: null, status, attempts: 0, escalated: false, route_reason: null, tokens_in: 0, tokens_out: 0,
  tokens_estimated: false, cost_usd: 0, paid_equiv_usd: 0, commit_ref: null, started_at: null, ended_at: null, detail: {},
});

describe("store.applyBatch", () => {
  it("applies a whole batch in one update", () => {
    let renders = 0;
    const un = useApp.subscribe(() => renders++);
    useApp.getState().applyBatch([
      { type: "run", run: run("running") },
      { type: "step", step: step("s1", "running") },
      { type: "log", run_id: "r1", step_id: "s1", line: { seq: 1, ts: 0, kind: "out", text: "hi" } },
      { type: "approval", request: { id: "a1", run_id: "r1", step_id: "s1", kind: "paid", title: "?", body: "", options: [] } },
      { type: "step", step: step("s1", "passed") },
    ]);
    un();
    expect(renders).toBe(1);
    const s = useApp.getState();
    expect(s.runs.r1.status).toBe("running");
    expect(s.steps.r1.s1.status).toBe("passed");
    expect(s.approvals).toHaveLength(1);
    expect(logStore.get("s1")[0].text).toBe("hi");
    useApp.getState().applyBatch([{ type: "approval_done", id: "a1" }]);
    expect(useApp.getState().approvals).toHaveLength(0);
  });
});
