import { buildSuggestions } from "./insights";

describe("routing insights", () => {
  it("flags weak cheap executors and strong local ones", () => {
    const s = buildSuggestions([
      { class: "low", tier: "local", model: "ollama/qwen2.5-coder:1.5b", budget_planning: false, first_try_passes: 1, total: 6 },
      { class: "low", tier: "local", model: "ollama/qwen2.5-coder:7b", budget_planning: false, first_try_passes: 9, total: 10 },
      { class: "high", tier: "premium", model: "cursor", budget_planning: false, first_try_passes: 1, total: 1 },
    ]);
    expect(s.some((x) => x.includes("1.5b") && x.includes("17%"))).toBe(true);
    expect(s.some((x) => x.includes("7b") && x.includes("Keep it first"))).toBe(true);
    expect(s.some((x) => x.includes("cursor"))).toBe(false); // too few samples
  });
});
