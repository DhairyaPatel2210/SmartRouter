import { compileWorkflow, templateFromMode } from "./model";
import type { ModeDef } from "../lib/types";

const base: Omit<ModeDef, "id" | "display_name" | "high" | "low" | "trivial" | "review" | "escalation"> = {
  builtin: true,
  description: "",
  unclear_as: "high",
  attempts_per_executor: 2,
  short_plan: false,
};

const balanced: ModeDef = { ...base, id: "balanced", display_name: "Balanced", high: "paid", low: "cheap", trivial: "cheap", review: true, escalation: "auto" };
const cost: ModeDef = { ...base, id: "cost", display_name: "Cost", high: "cheap", low: "cheap", trivial: "cheap", review: false, escalation: "approval", unclear_as: "low", short_plan: true };

describe("workflow model", () => {
  it("round-trips the built-in modes through templates", () => {
    for (const m of [balanced, cost]) {
      const c = compileWorkflow(templateFromMode(m));
      expect(c.high).toBe(m.high);
      expect(c.low).toBe(m.low);
      expect(c.trivial).toBe(m.trivial);
      expect(c.review).toBe(m.review);
      expect(c.escalation).toBe(m.escalation);
      expect(c.short_plan).toBe(m.short_plan);
      expect(c.id).toBe(`wf-template-${m.id}`);
    }
  });

  it("an edited graph changes the compiled mode", () => {
    const w = templateFromMode(balanced);
    w.nodes = w.nodes.map((n) => (n.id === "exec-low" ? { ...n, data: { ...n.data, tier: "premium" as const } } : n)).filter((n) => n.data.kind !== "review");
    const c = compileWorkflow(w);
    expect(c.low).toBe("paid");
    expect(c.review).toBe(false);
  });
});
